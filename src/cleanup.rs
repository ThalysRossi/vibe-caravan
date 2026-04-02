use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::WololoError;
use crate::models::batch::Batch;
use crate::models::state::{BatchPhase, JournalEntry, MigrationState};

pub fn cleanup_batch(
    batch: &Batch,
    source_root: &Path,
    state: &mut MigrationState,
    execution_context: &str,
) -> Result<(), WololoError> {
    let batch_state = state.batch(&batch.id).ok_or_else(|| {
        WololoError::InvalidArguments(format!(
            "missing batch state for {} before cleanup",
            batch.id
        ))
    })?;

    if !batch_state.verification_passed {
        return Err(WololoError::InvalidArguments(
            "deletion blocked because verification did not pass".to_string(),
        ));
    }
    if !batch_state.approved_for_delete {
        return Err(WololoError::InvalidArguments(
            "deletion blocked because batch is not approved".to_string(),
        ));
    }
    if batch_state.deleted {
        return Ok(());
    }

    for file in &batch.files {
        let path = source_root.join(&file.relative_path);
        if path.exists() {
            fs::remove_file(&path).map_err(|err| {
                WololoError::InvalidArguments(format!(
                    "failed to delete source file {}: {err}",
                    path.display()
                ))
            })?;
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
