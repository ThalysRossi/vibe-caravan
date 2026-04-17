use std::path::Path;

use crate::error::{CaravanError, VerificationFailure};
use crate::models::batch::Batch;
use crate::models::state::{BatchPhase, MigrationState};
use crate::{transfer, verify};

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
    transfer::transfer_batch_with_progress(
        batch,
        op.source_root,
        op.dest_root,
        op.copy_backend,
        &mut progress,
    )?;

    current_state.phase = BatchPhase::CopyCompleted;
    if op.reset_verification_passed {
        current_state.verification_passed = false;
    }
    state.upsert_batch(current_state);
    persist_state(state)?;

    Ok(())
}

pub(crate) fn verify_batch_with_state_updates(
    batch: &Batch,
    source_root: &Path,
    dest_root: &Path,
    state: &mut MigrationState,
    persist_state: &mut dyn FnMut(&MigrationState) -> Result<(), CaravanError>,
    missing_state_error: &dyn Fn(&str) -> CaravanError,
) -> Result<(), CaravanError> {
    let mut progress = crate::progress::TerminalProgress::new();
    let verification_report =
        verify::verify_batch_with_progress(batch, source_root, dest_root, &mut progress)?;

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
