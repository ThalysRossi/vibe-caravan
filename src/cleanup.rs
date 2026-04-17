use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::state::{BatchPhase, JournalEntry, MigrationState};

pub trait FileRemover {
    fn remove_file(&self, path: &Path) -> std::io::Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FsFileRemover;

impl FileRemover for FsFileRemover {
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_file(path)
    }
}

pub fn cleanup_batch(
    batch: &Batch,
    source_root: &Path,
    state: &mut MigrationState,
    execution_context: &str,
) -> Result<(), CaravanError> {
    cleanup_batch_with_remover(batch, source_root, state, execution_context, &FsFileRemover)
}

pub fn cleanup_batch_with_remover(
    batch: &Batch,
    source_root: &Path,
    state: &mut MigrationState,
    execution_context: &str,
    remover: &dyn FileRemover,
) -> Result<(), CaravanError> {
    let batch_state = state.batch(&batch.id).cloned().ok_or_else(|| {
        CaravanError::StateCorrupt(format!(
            "missing batch state for {} before cleanup",
            batch.id
        ))
    })?;

    if !batch_state.verification_passed {
        return Err(CaravanError::PolicyBlocked(
            "deletion blocked because verification did not pass".to_string(),
        ));
    }
    if !batch_state.approved_for_delete {
        return Err(CaravanError::PolicyBlocked(
            "deletion blocked because batch is not approved".to_string(),
        ));
    }
    if batch_state.deleted {
        return Ok(());
    }

    state.journal.push(JournalEntry {
        event: "delete_started".to_string(),
        batch_id: batch.id.clone(),
        timestamp_unix_secs: now_unix_secs(),
        context: execution_context.to_string(),
    });

    for file in &batch.files {
        let path = source_root.join(&file.relative_path);
        if path.exists() {
            if let Err(err) = remover.remove_file(&path) {
                let mut failed = batch_state.clone();
                failed.phase = BatchPhase::Failed;
                failed.deleted = false;
                state.upsert_batch(failed);
                state.journal.push(JournalEntry {
                    event: "delete_failed".to_string(),
                    batch_id: batch.id.clone(),
                    timestamp_unix_secs: now_unix_secs(),
                    context: format!(
                        "{} | file={} | error={}",
                        execution_context,
                        path.display(),
                        err
                    ),
                });
                return Err(CaravanError::IoContext {
                    context: format!("failed to delete source file {}", path.display()),
                    source: err,
                });
            }
        }
    }

    let mut updated = batch_state.clone();
    updated.deleted = true;
    updated.phase = BatchPhase::DeleteCompleted;
    state.upsert_batch(updated);
    state.journal.push(JournalEntry {
        event: "delete_completed".to_string(),
        batch_id: batch.id.clone(),
        timestamp_unix_secs: now_unix_secs(),
        context: execution_context.to_string(),
    });

    Ok(())
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
