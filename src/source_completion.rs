use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::atomic_write;
use crate::error::CaravanError;
use crate::migration_registry;
use crate::models::batch::Batch;
use crate::models::file_entry::FileEntry;
use crate::models::state::{BatchPhase, BatchState, CompletedFileIdentity};
use crate::{state_store, verify};

const LEDGER_FORMAT_VERSION: u32 = 1;
const LEDGER_FILENAME: &str = "source_completed_files.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashedFileEntry {
    pub relative_path: PathBuf,
    pub size_bytes: u64,
    pub blake3_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCompletionLedger {
    pub format_version: u32,
    pub entries: Vec<CompletedFileIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilteredSourceEntries {
    pub entries_to_plan: Vec<FileEntry>,
    pub skipped_completed_files: Vec<CompletedFileIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FileIdentityKey {
    mode: String,
    relative_path: PathBuf,
    size_bytes: u64,
    blake3_hash: String,
}

impl SourceCompletionLedger {
    pub fn new() -> Self {
        Self {
            format_version: LEDGER_FORMAT_VERSION,
            entries: Vec::new(),
        }
    }
}

impl Default for SourceCompletionLedger {
    fn default() -> Self {
        Self::new()
    }
}

pub fn ledger_path(source_root: &Path) -> PathBuf {
    migration_registry::state_dir_in_source(source_root).join(LEDGER_FILENAME)
}

pub fn load_ledger(source_root: &Path) -> Result<SourceCompletionLedger, CaravanError> {
    let path = ledger_path(source_root);
    if !path.exists() {
        return Ok(SourceCompletionLedger::new());
    }

    let payload = fs::read_to_string(&path).map_err(|source| CaravanError::StateRead {
        path: path.clone(),
        source,
    })?;
    let ledger: SourceCompletionLedger =
        serde_json::from_str(&payload).map_err(|source| CaravanError::StateParse {
            path: path.clone(),
            source,
        })?;

    if ledger.format_version != LEDGER_FORMAT_VERSION {
        return Err(CaravanError::StateCorrupt(format!(
            "unsupported source completion ledger format version {} in {}",
            ledger.format_version,
            path.display()
        )));
    }

    Ok(ledger)
}

pub fn persist_ledger(
    source_root: &Path,
    ledger: &SourceCompletionLedger,
) -> Result<(), CaravanError> {
    let payload = serde_json::to_vec_pretty(ledger).map_err(|err| {
        CaravanError::StateCorrupt(format!(
            "failed to serialize source completion ledger: {err}"
        ))
    })?;
    atomic_write::write_bytes(
        &ledger_path(source_root),
        &payload,
        "source completion ledger",
    )
}

pub fn hash_source_entries(
    source_root: &Path,
    entries: &[FileEntry],
) -> Result<Vec<HashedFileEntry>, CaravanError> {
    let mut hashed_entries = Vec::with_capacity(entries.len());
    for entry in entries {
        let source_path = source_root.join(&entry.relative_path);
        let blake3_hash = hash_file_hex(&source_path)?;
        hashed_entries.push(HashedFileEntry {
            relative_path: entry.relative_path.clone(),
            size_bytes: entry.size_bytes,
            blake3_hash,
        });
    }
    Ok(hashed_entries)
}

pub fn filter_entries_for_new_migration(
    mode: &str,
    entries: Vec<FileEntry>,
    hashed_entries: &[HashedFileEntry],
    ledger: &SourceCompletionLedger,
) -> Result<FilteredSourceEntries, CaravanError> {
    let completed_keys: HashSet<CompletedFileIdentity> = ledger.entries.iter().cloned().collect();
    let hashed_by_path: HashMap<&Path, &HashedFileEntry> = hashed_entries
        .iter()
        .map(|entry| (entry.relative_path.as_path(), entry))
        .collect();

    let mut entries_to_plan = Vec::new();
    let mut skipped_completed_files = Vec::new();
    for entry in entries {
        let hashed = hashed_by_path
            .get(entry.relative_path.as_path())
            .ok_or_else(|| missing_hashed_entry_error(&entry.relative_path))?;
        let identity = identity_from_hashed_entry(mode, hashed);
        if completed_keys.contains(&identity) {
            skipped_completed_files.push(identity);
        } else {
            entries_to_plan.push(entry);
        }
    }

    Ok(FilteredSourceEntries {
        entries_to_plan,
        skipped_completed_files,
    })
}

pub fn filter_entries_for_persisted_skips(
    mode: &str,
    entries: Vec<FileEntry>,
    hashed_entries: &[HashedFileEntry],
    skipped_completed_files: &[CompletedFileIdentity],
) -> Result<Vec<FileEntry>, CaravanError> {
    let hashed_by_path: HashMap<&Path, &HashedFileEntry> = hashed_entries
        .iter()
        .map(|entry| (entry.relative_path.as_path(), entry))
        .collect();
    let skipped_keys: HashSet<FileIdentityKey> = skipped_completed_files
        .iter()
        .map(file_key_from_completed_identity)
        .collect();

    for skipped in skipped_completed_files {
        if skipped.mode != mode {
            return Err(CaravanError::StateCorrupt(format!(
                "skipped completed file '{}' belongs to mode '{}' but current state mode is '{}'",
                skipped.relative_path.display(),
                skipped.mode,
                mode
            )));
        }
        let Some(current) = hashed_by_path.get(skipped.relative_path.as_path()) else {
            return Err(CaravanError::InvalidArguments(format!(
                "source drift detected for skipped completed file '{}': file is missing",
                skipped.relative_path.display()
            )));
        };
        let current_identity = CompletedFileIdentity {
            mode: skipped.mode.clone(),
            relative_path: current.relative_path.clone(),
            size_bytes: current.size_bytes,
            blake3_hash: current.blake3_hash.clone(),
        };
        if current_identity != *skipped {
            return Err(CaravanError::InvalidArguments(format!(
                "source drift detected for skipped completed file '{}': identity changed",
                skipped.relative_path.display()
            )));
        }
    }

    let mut filtered_entries = Vec::new();
    for entry in entries {
        let hashed = hashed_by_path
            .get(entry.relative_path.as_path())
            .ok_or_else(|| missing_hashed_entry_error(&entry.relative_path))?;
        let matching_skipped_key = skipped_keys.contains(&file_key_from_hashed_entry(mode, hashed));
        if !matching_skipped_key {
            filtered_entries.push(entry);
        }
    }

    Ok(filtered_entries)
}

fn missing_hashed_entry_error(relative_path: &Path) -> CaravanError {
    CaravanError::StateCorrupt(format!(
        "hashed source entries are missing scanned file '{}'",
        relative_path.display()
    ))
}

pub fn backfill_ledger_from_existing_states(
    source_root: &Path,
    mode: &str,
    hashed_entries: &[HashedFileEntry],
) -> Result<usize, CaravanError> {
    let state_dir = migration_registry::state_dir_in_source(source_root);
    if !state_dir.exists() {
        return Ok(0);
    }

    let mut ledger = load_ledger(source_root)?;
    let initial_len = ledger.entries.len();
    let mut completed_keys: HashSet<CompletedFileIdentity> =
        ledger.entries.iter().cloned().collect();
    let hashed_by_path: HashMap<&Path, &HashedFileEntry> = hashed_entries
        .iter()
        .map(|entry| (entry.relative_path.as_path(), entry))
        .collect();

    let source_identity = normalized_path(source_root);
    for state_path in migration_state_paths(&state_dir)? {
        let state = state_store::load_state(&state_path)?;
        if state.mode != mode || normalized_path(Path::new(&state.source)) != source_identity {
            continue;
        }

        for batch_state in state
            .batches
            .iter()
            .filter(|batch| eligible_batch_state(batch))
        {
            let Some(planned_batch) = state.planned_batch(&batch_state.batch_id) else {
                eprintln!(
                    "[WARNING] cannot backfill completed files for {} from {}: missing planned batch manifest",
                    batch_state.batch_id,
                    state_path.display()
                );
                continue;
            };

            for planned_file in &planned_batch.files {
                let Some(hashed) = hashed_by_path.get(planned_file.relative_path.as_path()) else {
                    continue;
                };
                if hashed.size_bytes != planned_file.size_bytes {
                    continue;
                }

                let identity = identity_from_hashed_entry(mode, hashed);
                if completed_keys.insert(identity.clone()) {
                    ledger.entries.push(identity);
                }
            }
        }
    }

    if ledger.entries.len() != initial_len {
        persist_ledger(source_root, &ledger)?;
    }

    Ok(ledger.entries.len().saturating_sub(initial_len))
}

pub fn mark_batch_completed(
    source_root: &Path,
    mode: &str,
    batch: &Batch,
) -> Result<(), CaravanError> {
    let identities = identities_from_batch(source_root, mode, batch)?;
    upsert_identities(source_root, &identities)
}

pub fn remove_batch_completed(
    source_root: &Path,
    mode: &str,
    batch: &Batch,
) -> Result<(), CaravanError> {
    if batch.files.is_empty() {
        return Ok(());
    }

    let paths_to_remove: HashSet<PathBuf> = batch
        .files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect();
    let mut ledger = load_ledger(source_root)?;
    let original_len = ledger.entries.len();
    ledger.entries.retain(|identity| {
        identity.mode != mode || !paths_to_remove.contains(&identity.relative_path)
    });
    if ledger.entries.len() != original_len {
        persist_ledger(source_root, &ledger)?;
    }
    Ok(())
}

fn identities_from_batch(
    source_root: &Path,
    mode: &str,
    batch: &Batch,
) -> Result<Vec<CompletedFileIdentity>, CaravanError> {
    let mut identities = Vec::with_capacity(batch.files.len());
    for file in &batch.files {
        let source_path = source_root.join(&file.relative_path);
        identities.push(CompletedFileIdentity {
            mode: mode.to_string(),
            relative_path: file.relative_path.clone(),
            size_bytes: file.size_bytes,
            blake3_hash: hash_file_hex(&source_path)?,
        });
    }
    Ok(identities)
}

fn upsert_identities(
    source_root: &Path,
    identities: &[CompletedFileIdentity],
) -> Result<(), CaravanError> {
    if identities.is_empty() {
        return Ok(());
    }

    let mut ledger = load_ledger(source_root)?;
    let mut completed_keys: HashSet<CompletedFileIdentity> =
        ledger.entries.iter().cloned().collect();
    for identity in identities {
        if completed_keys.insert(identity.clone()) {
            ledger.entries.push(identity.clone());
        }
    }
    persist_ledger(source_root, &ledger)
}

fn identity_from_hashed_entry(mode: &str, entry: &HashedFileEntry) -> CompletedFileIdentity {
    CompletedFileIdentity {
        mode: mode.to_string(),
        relative_path: entry.relative_path.clone(),
        size_bytes: entry.size_bytes,
        blake3_hash: entry.blake3_hash.clone(),
    }
}

fn file_key_from_completed_identity(identity: &CompletedFileIdentity) -> FileIdentityKey {
    FileIdentityKey {
        mode: identity.mode.clone(),
        relative_path: identity.relative_path.clone(),
        size_bytes: identity.size_bytes,
        blake3_hash: identity.blake3_hash.clone(),
    }
}

fn file_key_from_hashed_entry(mode: &str, entry: &HashedFileEntry) -> FileIdentityKey {
    FileIdentityKey {
        mode: mode.to_string(),
        relative_path: entry.relative_path.clone(),
        size_bytes: entry.size_bytes,
        blake3_hash: entry.blake3_hash.clone(),
    }
}

fn hash_file_hex(path: &Path) -> Result<String, CaravanError> {
    Ok(hex_lower(&verify::digest_file(path)?))
}

fn hex_lower(bytes: &[u8; 32]) -> String {
    let mut rendered = String::with_capacity(64);
    for byte in bytes {
        rendered.push_str(&format!("{byte:02x}"));
    }
    rendered
}

fn migration_state_paths(state_dir: &Path) -> Result<Vec<PathBuf>, CaravanError> {
    let mut paths = fs::read_dir(state_dir)
        .map_err(|source| CaravanError::IoContext {
            context: format!(
                "failed to enumerate source completion backfill states in {}",
                state_dir.display()
            ),
            source,
        })?
        .map(|entry| {
            entry
                .map(|dir_entry| dir_entry.path())
                .map_err(|source| CaravanError::IoContext {
                    context: format!(
                        "failed to read source completion backfill state entry in {}",
                        state_dir.display()
                    ),
                    source,
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    paths.retain(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("migration_") && name.ends_with(".json"))
    });
    paths.sort();
    Ok(paths)
}

fn eligible_batch_state(batch_state: &BatchState) -> bool {
    match batch_state.phase {
        BatchPhase::CopyCompleted
        | BatchPhase::ApprovedForDelete
        | BatchPhase::DeleteCompleted
        | BatchPhase::SnapshotCompleted => true,
        BatchPhase::VerifyCompleted => batch_state.verification_passed,
        BatchPhase::Planned | BatchPhase::CopyStarted | BatchPhase::Failed => false,
    }
}

fn normalized_path(path: &Path) -> String {
    let normalized_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let rendered = normalized_path.to_string_lossy().replace('\\', "/");
    #[cfg(target_os = "windows")]
    {
        return rendered.to_ascii_lowercase();
    }
    #[cfg(target_os = "linux")]
    {
        rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(relative_path: &str, hash: &str) -> HashedFileEntry {
        HashedFileEntry {
            relative_path: PathBuf::from(relative_path),
            size_bytes: 4,
            blake3_hash: hash.to_string(),
        }
    }

    #[test]
    fn filter_entries_for_new_migration_skips_exact_ledger_matches() {
        let entries = vec![
            FileEntry {
                relative_path: PathBuf::from("a.txt"),
                size_bytes: 4,
                modified_time: None,
            },
            FileEntry {
                relative_path: PathBuf::from("b.txt"),
                size_bytes: 4,
                modified_time: None,
            },
        ];
        let hashed = vec![sample_entry("a.txt", "aaaa"), sample_entry("b.txt", "bbbb")];
        let ledger = SourceCompletionLedger {
            format_version: LEDGER_FORMAT_VERSION,
            entries: vec![CompletedFileIdentity {
                mode: "staging".to_string(),
                relative_path: PathBuf::from("a.txt"),
                size_bytes: 4,
                blake3_hash: "aaaa".to_string(),
            }],
        };

        let filtered = filter_entries_for_new_migration("staging", entries, &hashed, &ledger)
            .expect("filtering should succeed");

        assert_eq!(filtered.entries_to_plan.len(), 1);
        assert_eq!(
            filtered.entries_to_plan[0].relative_path,
            PathBuf::from("b.txt")
        );
        assert_eq!(filtered.skipped_completed_files.len(), 1);
    }

    #[test]
    fn filter_entries_for_persisted_skips_detects_changed_hash() {
        let entries = vec![FileEntry {
            relative_path: PathBuf::from("a.txt"),
            size_bytes: 4,
            modified_time: None,
        }];
        let hashed = vec![sample_entry("a.txt", "changed")];
        let skipped = vec![CompletedFileIdentity {
            mode: "staging".to_string(),
            relative_path: PathBuf::from("a.txt"),
            size_bytes: 4,
            blake3_hash: "original".to_string(),
        }];

        let err = filter_entries_for_persisted_skips("staging", entries, &hashed, &skipped)
            .expect_err("changed skipped file should be source drift");

        assert!(err.to_string().contains("source drift detected"));
    }
}
