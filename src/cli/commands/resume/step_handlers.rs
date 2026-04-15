use std::path::Path;

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::state::MigrationState;
use crate::{resume as resume_ops, state_store, transfer};

use super::super::shared::{
    copy_batch_with_state_updates, ensure_destination_capacity, print_resume_continue_to_deletion,
    print_resume_processing_batch_banner, print_resume_skip_already_completed,
    print_resume_verification_passed, verify_batch_with_state_updates, CopyBatchOp,
};

fn verify_batch_for_resume(
    batch: &Batch,
    config: &TransferConfig,
    state: &mut MigrationState,
    state_path: &Path,
) -> Result<(), CaravanError> {
    println!("Verifying {}...", batch.id);
    let mut persist_state =
        |current_state: &MigrationState| state_store::persist_state(state_path, current_state);
    verify_batch_with_state_updates(
        batch,
        &config.source,
        &config.dest,
        &config.verification,
        state,
        &mut persist_state,
        &|batch_id| {
            CaravanError::InvalidArguments(format!(
                "batch {} disappeared from state during verification",
                batch_id
            ))
        },
    )?;

    print_resume_verification_passed();
    Ok(())
}

fn copy_batch_for_resume(
    batch: &Batch,
    config: &TransferConfig,
    state: &mut MigrationState,
    state_path: &Path,
    copy_backend: &transfer::LocalFsCopyBackend,
) -> Result<(), CaravanError> {
    print_resume_processing_batch_banner(batch);

    ensure_destination_capacity(&config.dest, batch.total_bytes)?;
    let mut persist_state =
        |current_state: &MigrationState| state_store::persist_state(state_path, current_state);
    copy_batch_with_state_updates(
        batch,
        state,
        CopyBatchOp {
            source_root: &config.source,
            dest_root: &config.dest,
            copy_backend,
            reset_verification_passed: false,
        },
        &mut persist_state,
        &|batch_id| {
            CaravanError::InvalidArguments(format!(
                "batch {} disappeared from state during copy",
                batch_id
            ))
        },
    )?;

    Ok(())
}

pub(super) fn execute_resume_step(
    step: resume_ops::ResumeStepPlan,
    batch: &Batch,
    config: &TransferConfig,
    state: &mut MigrationState,
    state_path: &Path,
    copy_backend: &transfer::LocalFsCopyBackend,
) -> Result<(), CaravanError> {
    match step {
        resume_ops::ResumeStepPlan::BatchFullyCompleted
        | resume_ops::ResumeStepPlan::PostDeleteSnapshot => {
            print_resume_skip_already_completed(&batch.id);
            Ok(())
        }
        resume_ops::ResumeStepPlan::ConflictOperatorReview { reason } => {
            Err(CaravanError::InvalidArguments(format!(
                "batch {} requires operator review before continuing: {}",
                batch.id, reason
            )))
        }
        resume_ops::ResumeStepPlan::BlockedFailedVerification => {
            Err(CaravanError::InvalidArguments(format!(
                "batch {} failed verification and requires operator review before continuing",
                batch.id
            )))
        }
        resume_ops::ResumeStepPlan::DeleteSource
        | resume_ops::ResumeStepPlan::PendingDeleteApproval => {
            print_resume_continue_to_deletion(&batch.id);
            Ok(())
        }
        resume_ops::ResumeStepPlan::VerifyBatch => {
            verify_batch_for_resume(batch, config, state, state_path)
        }
        resume_ops::ResumeStepPlan::CopyBatch => {
            copy_batch_for_resume(batch, config, state, state_path, copy_backend)?;
            verify_batch_for_resume(batch, config, state, state_path)
        }
    }
}
