use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::conflict::ConflictReport;
use crate::error::{CaravanError, VerificationFailure};
use crate::models::batch::Batch;
use crate::models::file_entry::FileEntry;
use crate::models::state::{BatchPhase, BatchState, JournalEntry, MigrationState};
use crate::{transfer, verify};

use super::output::print_verification_failed;

pub(crate) struct CopyBatchOp<'a> {
    pub source_root: &'a Path,
    pub dest_root: &'a Path,
    pub copy_backend: &'a transfer::LocalFsCopyBackend,
    pub reset_verification_passed: bool,
}

pub(crate) fn copy_batch_with_state_updates(
    batch: &Batch,
    state: &mut MigrationState,
    op: CopyBatchOp<'_>,
    persist_state: &mut dyn FnMut(&MigrationState) -> Result<(), CaravanError>,
    check_shutdown: &mut dyn FnMut() -> Result<(), CaravanError>,
    missing_state_error: &dyn Fn(&str) -> CaravanError,
) -> Result<(), CaravanError> {
    let mut current_state = state
        .batch(&batch.id)
        .cloned()
        .ok_or_else(|| missing_state_error(&batch.id))?;

    current_state.phase = BatchPhase::CopyStarted;
    state.upsert_batch(current_state.clone());
    persist_state(state)?;

    let mut progress = crate::progress::TerminalProgress::new();
    op.copy_backend.copy_batch(
        batch,
        op.source_root,
        op.dest_root,
        &mut progress,
        check_shutdown,
    )?;

    current_state.phase = BatchPhase::CopyCompleted;
    if op.reset_verification_passed {
        current_state.verification_passed = false;
    }
    state.upsert_batch(current_state);
    persist_state(state)?;

    Ok(())
}

pub(crate) fn non_conflicting_subset_batch(
    batch: &Batch,
    destination_root: &Path,
    report: &ConflictReport,
) -> Batch {
    let conflicting_paths: std::collections::HashSet<std::path::PathBuf> =
        report.existing_files.iter().cloned().collect();
    let files: Vec<FileEntry> = batch
        .files
        .iter()
        .filter(|file_entry| {
            let destination_path = destination_root.join(&file_entry.relative_path);
            !conflicting_paths.contains(&destination_path)
        })
        .cloned()
        .collect();
    let total_bytes = files.iter().map(|file_entry| file_entry.size_bytes).sum();

    Batch {
        id: batch.id.clone(),
        file_count: files.len(),
        total_bytes,
        files,
    }
}

pub(crate) fn mark_batch_failed_for_conflicts(
    batch: &Batch,
    state: &mut MigrationState,
    conflict_report: &ConflictReport,
    copied_non_conflicting_files: usize,
    missing_state: &dyn Fn(&str) -> Result<BatchState, CaravanError>,
) -> Result<(), CaravanError> {
    let mut current_batch_state = state
        .batch(&batch.id)
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| missing_state(&batch.id))?;
    current_batch_state.phase = BatchPhase::Failed;
    current_batch_state.verification_passed = false;
    state.upsert_batch(current_batch_state);
    state.journal.push(JournalEntry {
        event: "copy_failed_conflict".to_string(),
        batch_id: batch.id.clone(),
        timestamp_unix_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        context: format!(
            "naming_conflicts={} size_mismatches={} copied_non_conflicting_files={} skipped_conflicting_files={}",
            conflict_report.total_conflicts,
            conflict_report.size_mismatches.len(),
            copied_non_conflicting_files,
            conflict_report.total_conflicts
        ),
    });

    Ok(())
}

pub(crate) fn handle_verification_error(
    batch: &Batch,
    source_root: &Path,
    mode: &str,
    err: &CaravanError,
) -> Result<(), CaravanError> {
    if let CaravanError::VerificationFailed(failure) = err {
        print_verification_failed(failure);
        crate::source_completion::remove_batch_completed(source_root, mode, batch).map_err(
            |remove_err| {
                CaravanError::StateCorrupt(format!(
                    "verification failed for {} and completed-file ledger cleanup failed: {}",
                    batch.id, remove_err
                ))
            },
        )?;
    }

    Ok(())
}

pub(crate) fn verify_batch_with_state_updates(
    batch: &Batch,
    source_root: &Path,
    dest_root: &Path,
    state: &mut MigrationState,
    persist_state: &mut dyn FnMut(&MigrationState) -> Result<(), CaravanError>,
    check_shutdown: &mut dyn FnMut() -> Result<(), CaravanError>,
    missing_state_error: &dyn Fn(&str) -> CaravanError,
) -> Result<(), CaravanError> {
    let mut progress = crate::progress::TerminalProgress::new();
    let verification_report = verify::verify_batch_with_progress(
        batch,
        source_root,
        dest_root,
        &mut progress,
        check_shutdown,
    )?;

    let mut current_state = state
        .batch(&batch.id)
        .cloned()
        .ok_or_else(|| missing_state_error(&batch.id))?;
    current_state.phase = BatchPhase::VerifyCompleted;
    current_state.verification_passed =
        verification_report.status == crate::models::verification::VerificationStatus::Pass;
    state.upsert_batch(current_state.clone());
    persist_state(state)?;

    if !current_state.verification_passed {
        return Err(CaravanError::VerificationFailed(
            VerificationFailure::from_report(verification_report),
        ));
    }

    Ok(())
}
