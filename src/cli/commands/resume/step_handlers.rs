use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::state::MigrationState;
use crate::resume as resume_ops;

use super::super::shared::{
    copy_batch_with_state_updates, ensure_destination_capacity, print_resume_continue_to_deletion,
    print_resume_processing_batch_banner, print_resume_skip_already_completed,
    print_resume_verification_passed, print_verification_failed, verify_batch_with_state_updates,
    CopyBatchOp,
};
use super::context::ResumeContext;

fn verify_batch_for_resume(
    batch: &Batch,
    context: &ResumeContext<'_>,
    state: &mut MigrationState,
) -> Result<(), CaravanError> {
    println!("Verifying {}...", batch.id);
    let mut persist_state = |current_state: &MigrationState| context.persist_state(current_state);
    if let Err(err) = verify_batch_with_state_updates(
        batch,
        &context.config.source,
        &context.config.dest,
        state,
        &mut persist_state,
        &|batch_id| {
            CaravanError::StateCorrupt(format!(
                "batch {} disappeared from state during verification",
                batch_id
            ))
        },
    ) {
        if let CaravanError::VerificationFailed(failure) = &err {
            print_verification_failed(failure);
        }
        return Err(err);
    }

    print_resume_verification_passed();
    Ok(())
}

fn copy_batch_for_resume(
    batch: &Batch,
    context: &ResumeContext<'_>,
    state: &mut MigrationState,
) -> Result<(), CaravanError> {
    print_resume_processing_batch_banner(batch);

    ensure_destination_capacity(&context.config.dest, batch.total_bytes)?;
    let mut persist_state = |current_state: &MigrationState| context.persist_state(current_state);
    copy_batch_with_state_updates(
        batch,
        state,
        CopyBatchOp {
            source_root: &context.config.source,
            dest_root: &context.config.dest,
            copy_backend: &context.copy_backend,
            reset_verification_passed: false,
        },
        &mut persist_state,
        &|batch_id| {
            CaravanError::StateCorrupt(format!(
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
    context: &ResumeContext<'_>,
    state: &mut MigrationState,
) -> Result<(), CaravanError> {
    match step {
        resume_ops::ResumeStepPlan::BatchFullyCompleted
        | resume_ops::ResumeStepPlan::PostDeleteSnapshot => {
            print_resume_skip_already_completed(&batch.id);
            Ok(())
        }
        resume_ops::ResumeStepPlan::ConflictOperatorReview { reason } => {
            Err(CaravanError::PolicyBlocked(format!(
                "batch {} requires operator review before continuing: {}",
                batch.id, reason
            )))
        }
        resume_ops::ResumeStepPlan::BlockedFailedVerification => {
            Err(CaravanError::PolicyBlocked(format!(
                "batch {} failed verification and requires operator review before continuing",
                batch.id
            )))
        }
        resume_ops::ResumeStepPlan::DeleteSource
        | resume_ops::ResumeStepPlan::PendingDeleteApproval => {
            print_resume_continue_to_deletion(&batch.id);
            Ok(())
        }
        resume_ops::ResumeStepPlan::VerifyBatch => verify_batch_for_resume(batch, context, state),
        resume_ops::ResumeStepPlan::CopyBatch => {
            copy_batch_for_resume(batch, context, state)?;
            verify_batch_for_resume(batch, context, state)
        }
    }
}
