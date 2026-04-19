use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::ConflictPolicy;
use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::state::{BatchPhase, JournalEntry, MigrationState};
use crate::resume as resume_ops;

use super::super::shared::{
    CopyBatchOp, copy_batch_with_state_updates, ensure_destination_capacity,
    print_resume_continue_to_deletion, print_resume_processing_batch_banner,
    print_resume_skip_already_completed, print_resume_verification_passed,
    print_verification_failed, verify_batch_with_state_updates,
};
use super::context::ResumeContext;

fn effective_conflict_policy(context: &ResumeContext<'_>) -> ConflictPolicy {
    if context.config.recover_failed {
        ConflictPolicy::SkipFile
    } else {
        context.config.conflict_policy
    }
}

fn mark_batch_failed_for_conflicts(
    batch: &Batch,
    state: &mut MigrationState,
    context: &ResumeContext<'_>,
    conflict_report: &crate::conflict::ConflictReport,
    copied_non_conflicting_files: usize,
) -> Result<(), CaravanError> {
    let mut current_batch_state = state.batch(&batch.id).cloned().ok_or_else(|| {
        CaravanError::StateCorrupt(format!(
            "batch {} disappeared from state during conflict handling",
            batch.id
        ))
    })?;
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
    context.persist_state(state)
}

fn non_conflicting_subset_batch(
    batch: &Batch,
    destination_root: &std::path::Path,
    report: &crate::conflict::ConflictReport,
) -> Batch {
    let conflicting_paths: HashSet<std::path::PathBuf> =
        report.existing_files.iter().cloned().collect();
    let files = batch
        .files
        .iter()
        .filter(|file_entry| {
            let destination_path = destination_root.join(&file_entry.relative_path);
            !conflicting_paths.contains(&destination_path)
        })
        .cloned()
        .collect::<Vec<_>>();
    let total_bytes = files.iter().map(|file_entry| file_entry.size_bytes).sum();

    Batch {
        id: batch.id.clone(),
        file_count: files.len(),
        total_bytes,
        files,
    }
}

fn verify_batch_for_resume(
    batch: &Batch,
    context: &ResumeContext<'_>,
    state: &mut MigrationState,
) -> Result<(), CaravanError> {
    println!("Verifying {}...", batch.id);
    let mut persist_state = |current_state: &MigrationState| context.persist_state(current_state);
    let mut check_shutdown = || context.check_shutdown();
    if let Err(err) = verify_batch_with_state_updates(
        batch,
        &context.config.source,
        &context.config.dest,
        state,
        &mut persist_state,
        &mut check_shutdown,
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

    let conflict_report = crate::conflict::detect_batch_conflicts(batch, &context.config.dest)?;
    if conflict_report.has_conflicts {
        match effective_conflict_policy(context) {
            ConflictPolicy::SkipBatch => {
                mark_batch_failed_for_conflicts(batch, state, context, &conflict_report, 0)?;
                return Err(CaravanError::PolicyBlocked(format!(
                    "batch {} requires operator review before continuing: naming conflicts={} size_mismatches={}",
                    batch.id,
                    conflict_report.total_conflicts,
                    conflict_report.size_mismatches.len()
                )));
            }
            ConflictPolicy::SkipFile => {
                let copy_subset =
                    non_conflicting_subset_batch(batch, &context.config.dest, &conflict_report);
                if copy_subset.files.is_empty() {
                    mark_batch_failed_for_conflicts(batch, state, context, &conflict_report, 0)?;
                    return Err(CaravanError::PolicyBlocked(format!(
                        "batch {} requires operator review before continuing: naming conflicts={} size_mismatches={}",
                        batch.id,
                        conflict_report.total_conflicts,
                        conflict_report.size_mismatches.len()
                    )));
                }

                let mut persist_state =
                    |current_state: &MigrationState| context.persist_state(current_state);
                let mut check_shutdown = || context.check_shutdown();
                copy_batch_with_state_updates(
                    &copy_subset,
                    state,
                    CopyBatchOp {
                        source_root: &context.config.source,
                        dest_root: &context.config.dest,
                        copy_backend: &context.copy_backend,
                        reset_verification_passed: false,
                    },
                    &mut persist_state,
                    &mut check_shutdown,
                    &|batch_id| {
                        CaravanError::StateCorrupt(format!(
                            "batch {} disappeared from state during copy",
                            batch_id
                        ))
                    },
                )?;

                mark_batch_failed_for_conflicts(
                    batch,
                    state,
                    context,
                    &conflict_report,
                    copy_subset.file_count,
                )?;
                return Err(CaravanError::PolicyBlocked(format!(
                    "batch {} requires operator review before continuing: naming conflicts={} size_mismatches={}",
                    batch.id,
                    conflict_report.total_conflicts,
                    conflict_report.size_mismatches.len()
                )));
            }
        }
    }

    let mut persist_state = |current_state: &MigrationState| context.persist_state(current_state);
    let mut check_shutdown = || context.check_shutdown();
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
        &mut check_shutdown,
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
