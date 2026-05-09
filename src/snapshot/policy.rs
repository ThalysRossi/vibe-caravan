use crate::config::Mode;
use crate::error::CaravanError;
use crate::models::state::{BatchPhase, JournalEntry, MigrationState};

use super::backend::SnapshotBackend;
use super::now_unix_secs;
use super::request::SnapshotRequest;

pub fn snapshot_if_needed(
    request: SnapshotRequest<'_>,
    state: &mut MigrationState,
    backend: &dyn SnapshotBackend,
) -> Result<Option<String>, CaravanError> {
    let SnapshotRequest {
        mode,
        snapshot_every,
        completed_batch_count,
        batch_id,
        destination_root,
        snapshot_root,
    } = request;

    if mode == Mode::Staging {
        if snapshot_every.is_some() {
            return Err(CaravanError::InvalidArguments(
                "snapshots are only supported in migrate mode".to_string(),
            ));
        }
        return Ok(None);
    }

    let cadence = match snapshot_every {
        Some(value) if value > 0 => value,
        Some(_) => {
            return Err(CaravanError::InvalidArguments(
                "snapshot cadence must be greater than zero".to_string(),
            ));
        }
        None => return Ok(None),
    };

    if completed_batch_count == 0 || completed_batch_count % cadence != 0 {
        return Ok(None);
    }

    match backend.create_snapshot(destination_root, snapshot_root, batch_id) {
        Ok(snapshot_name) => {
            state.last_successful_snapshot_name = Some(snapshot_name.clone());
            if let Some(batch) = state.batches.iter_mut().find(|b| b.batch_id == batch_id) {
                batch.phase = BatchPhase::SnapshotCompleted;
            }
            state.journal.push(JournalEntry {
                event: "snapshot_completed".to_string(),
                batch_id: batch_id.to_string(),
                timestamp_unix_secs: now_unix_secs(),
                context: snapshot_name.clone(),
            });
            Ok(Some(snapshot_name))
        }
        Err(err) => {
            state.journal.push(JournalEntry {
                event: "snapshot_failed".to_string(),
                batch_id: batch_id.to_string(),
                timestamp_unix_secs: now_unix_secs(),
                context: err.to_string(),
            });
            Err(err)
        }
    }
}
