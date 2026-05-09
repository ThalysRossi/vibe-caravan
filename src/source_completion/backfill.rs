use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::CaravanError;
use crate::migration_registry;
use crate::models::state::{BatchPhase, BatchState, CompletedFileIdentity};
use crate::state_store;

use super::identity::{HashedFileEntry, identity_from_hashed_entry};
use super::ledger::{load_ledger, persist_ledger};

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
