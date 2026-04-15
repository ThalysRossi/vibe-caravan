use std::path::Path;

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::models;
use crate::models::state::{BatchPhase, MigrationState};
use crate::signal::{check_shutdown, ShutdownFlag};
use crate::{format, plan, resume as resume_ops, state_store, transfer, verify};

use super::super::shared::ensure_destination_capacity;

fn verify_batch_for_resume(
    batch: &crate::models::batch::Batch,
    config: &TransferConfig,
    state: &mut MigrationState,
    state_path: &Path,
) -> Result<(), CaravanError> {
    println!("Verifying {}...", batch.id);
    let mut progress = crate::progress::TerminalProgress::new();
    let verification_report = verify::verify_batch_with_progress(
        batch,
        &config.source,
        &config.dest,
        config.verification.clone(),
        &mut progress,
    )?;

    let mut current_state = state.batch(&batch.id).cloned().ok_or_else(|| {
        CaravanError::InvalidArguments(format!(
            "batch {} disappeared from state during verification",
            batch.id
        ))
    })?;
    current_state.phase = BatchPhase::VerifyCompleted;
    current_state.verification_passed =
        verification_report.status == models::verification::VerificationStatus::Pass;
    state.upsert_batch(current_state.clone());
    state_store::persist_state(state_path, state)?;

    if !current_state.verification_passed {
        eprintln!(
            "Verification failed: {}",
            verification_report.recommended_action
        );
        eprintln!("Missing: {:?}", verification_report.missing_files);
        eprintln!("Mismatched: {:?}", verification_report.mismatched_files);
        return Err(CaravanError::InvalidArguments(
            "verification failed".to_string(),
        ));
    }

    println!("✅ Verification passed!");
    Ok(())
}

fn copy_batch_for_resume(
    batch: &crate::models::batch::Batch,
    config: &TransferConfig,
    state: &mut MigrationState,
    state_path: &Path,
    copy_backend: &transfer::LocalFsCopyBackend,
) -> Result<(), CaravanError> {
    println!(
        "\n=== Processing {} ({} files, {}) ===",
        batch.id,
        batch.file_count,
        format::format_bytes(batch.total_bytes)
    );

    ensure_destination_capacity(&config.dest, batch.total_bytes)?;

    let mut current_state = state.batch(&batch.id).cloned().ok_or_else(|| {
        CaravanError::InvalidArguments(format!(
            "batch {} disappeared from state during copy",
            batch.id
        ))
    })?;
    current_state.phase = BatchPhase::CopyStarted;
    state.upsert_batch(current_state.clone());
    state_store::persist_state(state_path, state)?;

    let mut progress = crate::progress::TerminalProgress::new();
    transfer::transfer_batch_with_progress(
        batch,
        &config.source,
        &config.dest,
        copy_backend,
        &mut progress,
    )?;

    current_state.phase = BatchPhase::CopyCompleted;
    state.upsert_batch(current_state);
    state_store::persist_state(state_path, state)?;

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
        println!("⏭️  Skipping {}: already completed", batch_state.batch_id);
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
            println!("⏭️  Skipping {}: already completed", batch.id);
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
            println!(
                "✅ {} already verified, will continue to deletion phase",
                batch.id
            );
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
