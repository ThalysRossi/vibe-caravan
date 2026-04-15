use std::path::Path;

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::signal::{check_shutdown, ShutdownFlag};
use crate::{plan, resume as resume_ops, state_store, transfer};

use super::super::shared::{
    copy_batch_with_state_updates, ensure_destination_capacity, print_resume_continue_to_deletion,
    print_resume_processing_batch_banner, print_resume_skip_already_completed,
    print_resume_verification_passed, verify_batch_with_state_updates, CopyBatchOp,
};

fn verify_batch_for_resume(
    batch: &crate::models::batch::Batch,
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
    batch: &crate::models::batch::Batch,
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

fn process_resume_batch(
    batch_id: &str,
    config: &TransferConfig,
    state: &mut MigrationState,
    state_path: &Path,
    copy_backend: &transfer::LocalFsCopyBackend,
) -> Result<(), CaravanError> {
    let batch_state = state
        .batch(batch_id)
        .ok_or_else(|| {
            CaravanError::InvalidArguments(format!(
                "Batch {} disappeared from state during resume iteration",
                batch_id
            ))
        })?
        .clone();

    if batch_state.deleted {
        print_resume_skip_already_completed(&batch_state.batch_id);
        return Ok(());
    }

    let batch = plan::load_batch_definition(
        &config.source,
        &batch_state.batch_id,
        state.batch_size_bytes,
        state.max_files,
    )?;
    let recon = resume_ops::reconcile_batch_destination(&batch, &config.dest);
    let step = resume_ops::plan_resume_step(&batch_state, &recon, &batch);

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
            verify_batch_for_resume(&batch, config, state, state_path)
        }
        resume_ops::ResumeStepPlan::CopyBatch => {
            copy_batch_for_resume(&batch, config, state, state_path, copy_backend)?;
            verify_batch_for_resume(&batch, config, state, state_path)
        }
    }
}

pub(super) fn run_resume_batches(
    state: &mut MigrationState,
    config: &TransferConfig,
    state_path: &Path,
    shutdown_flag: &ShutdownFlag,
    copy_backend: &transfer::LocalFsCopyBackend,
) -> Result<(), CaravanError> {
    let batch_ids: Vec<String> = state.batches.iter().map(|b| b.batch_id.clone()).collect();

    for batch_id in batch_ids {
        check_shutdown(shutdown_flag)?;
        process_resume_batch(&batch_id, config, state, state_path, copy_backend)?;
    }

    Ok(())
}
