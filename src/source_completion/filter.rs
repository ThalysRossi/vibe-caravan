use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::error::CaravanError;
use crate::models::file_entry::FileEntry;
use crate::models::state::CompletedFileIdentity;

use super::identity::{
    FileIdentityKey, HashedFileEntry, file_key_from_completed_identity, file_key_from_hashed_entry,
    identity_from_hashed_entry,
};
use super::ledger::SourceCompletionLedger;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilteredSourceEntries {
    pub entries_to_plan: Vec<FileEntry>,
    pub skipped_completed_files: Vec<CompletedFileIdentity>,
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
        validate_skipped_file(mode, skipped, &hashed_by_path)?;
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

fn validate_skipped_file(
    mode: &str,
    skipped: &CompletedFileIdentity,
    hashed_by_path: &HashMap<&Path, &HashedFileEntry>,
) -> Result<(), CaravanError> {
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

    Ok(())
}

fn missing_hashed_entry_error(relative_path: &Path) -> CaravanError {
    CaravanError::StateCorrupt(format!(
        "hashed source entries are missing scanned file '{}'",
        relative_path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
        let mut ledger = SourceCompletionLedger::new();
        ledger.entries.push(CompletedFileIdentity {
            mode: "staging".to_string(),
            relative_path: PathBuf::from("a.txt"),
            size_bytes: 4,
            blake3_hash: "aaaa".to_string(),
        });

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
