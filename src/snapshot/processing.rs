use std::path::Path;

use crate::config::Mode;
use crate::error::CaravanError;
use crate::models::state::{BatchPhase, MigrationState};

use super::backend::SnapshotBackend;
use super::policy::snapshot_if_needed;
use super::request::SnapshotRequest;
use super::validation::validate_snapshot_configuration;

pub trait SnapshotProgressReporter {
    fn creating_snapshot(&mut self, batch_id: &str, deleted_batch_count: u32);
}

#[derive(Debug, Default)]
pub struct StderrSnapshotProgress;

impl SnapshotProgressReporter for StderrSnapshotProgress {
    fn creating_snapshot(&mut self, batch_id: &str, deleted_batch_count: u32) {
        eprintln!(
            "[SNAPSHOT] Creating snapshot after deleted batch count {} (batch={})...",
            deleted_batch_count, batch_id
        );
    }
}

pub fn process_pending_snapshots(
    mode: Mode,
    snapshot_every: Option<u32>,
    destination_root: &Path,
    snapshot_root: Option<&Path>,
    state: &mut MigrationState,
    backend: &dyn SnapshotBackend,
    persist_state: &mut dyn FnMut(&MigrationState) -> Result<(), CaravanError>,
) -> Result<(), CaravanError> {
    let mut progress = StderrSnapshotProgress;
    let mut no_interrupt = || Ok(());
    process_pending_snapshots_with_progress_and_interrupt(
        mode,
        snapshot_every,
        destination_root,
        snapshot_root,
        state,
        backend,
        persist_state,
        &mut progress,
        &mut no_interrupt,
    )
}

pub fn process_pending_snapshots_with_interrupt(
    mode: Mode,
    snapshot_every: Option<u32>,
    destination_root: &Path,
    snapshot_root: Option<&Path>,
    state: &mut MigrationState,
    backend: &dyn SnapshotBackend,
    persist_state: &mut dyn FnMut(&MigrationState) -> Result<(), CaravanError>,
    check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
) -> Result<(), CaravanError> {
    let mut progress = StderrSnapshotProgress;
    process_pending_snapshots_with_progress_and_interrupt(
        mode,
        snapshot_every,
        destination_root,
        snapshot_root,
        state,
        backend,
        persist_state,
        &mut progress,
        check_interrupt,
    )
}

pub fn process_pending_snapshots_with_progress(
    mode: Mode,
    snapshot_every: Option<u32>,
    destination_root: &Path,
    snapshot_root: Option<&Path>,
    state: &mut MigrationState,
    backend: &dyn SnapshotBackend,
    persist_state: &mut dyn FnMut(&MigrationState) -> Result<(), CaravanError>,
    progress: &mut dyn SnapshotProgressReporter,
) -> Result<(), CaravanError> {
    let mut no_interrupt = || Ok(());
    process_pending_snapshots_with_progress_and_interrupt(
        mode,
        snapshot_every,
        destination_root,
        snapshot_root,
        state,
        backend,
        persist_state,
        progress,
        &mut no_interrupt,
    )
}

pub fn process_pending_snapshots_with_progress_and_interrupt(
    mode: Mode,
    snapshot_every: Option<u32>,
    destination_root: &Path,
    snapshot_root: Option<&Path>,
    state: &mut MigrationState,
    backend: &dyn SnapshotBackend,
    persist_state: &mut dyn FnMut(&MigrationState) -> Result<(), CaravanError>,
    progress: &mut dyn SnapshotProgressReporter,
    check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
) -> Result<(), CaravanError> {
    validate_snapshot_configuration(
        mode.clone(),
        snapshot_every,
        destination_root,
        snapshot_root,
    )?;

    if snapshot_every.is_none() {
        return Ok(());
    }

    let mut deleted_batch_ids: Vec<String> = state
        .batches
        .iter()
        .filter(|batch| batch.deleted)
        .map(|batch| batch.batch_id.clone())
        .collect();
    deleted_batch_ids.sort();

    let mut deleted_batch_count = 0u32;
    for batch_id in deleted_batch_ids {
        check_interrupt()?;
        deleted_batch_count = deleted_batch_count.saturating_add(1);

        if state
            .batch(&batch_id)
            .map(|batch| batch.phase == BatchPhase::SnapshotCompleted)
            .unwrap_or(false)
        {
            continue;
        }

        if snapshot_is_due(snapshot_every, deleted_batch_count) {
            check_interrupt()?;
            progress.creating_snapshot(&batch_id, deleted_batch_count);
        }

        match snapshot_if_needed(
            SnapshotRequest {
                mode: mode.clone(),
                snapshot_every,
                completed_batch_count: deleted_batch_count,
                batch_id: &batch_id,
                destination_root,
                snapshot_root,
            },
            state,
            backend,
        ) {
            Ok(Some(snapshot_name)) => {
                eprintln!(
                    "[SNAPSHOT] Created '{}' after deleted batch count {} (batch={})",
                    snapshot_name, deleted_batch_count, batch_id
                );
                persist_state(state)?;
            }
            Ok(None) => {}
            Err(err) => {
                eprintln!(
                    "[WARNING] Snapshot failed for batch {} after deleted batch count {}: {}",
                    batch_id, deleted_batch_count, err
                );
                persist_state(state)?;
            }
        }
    }

    Ok(())
}

fn snapshot_is_due(snapshot_every: Option<u32>, deleted_batch_count: u32) -> bool {
    matches!(snapshot_every, Some(cadence) if cadence > 0 && deleted_batch_count % cadence == 0)
}
