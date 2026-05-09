use crate::models::batch::Batch;
use crate::models::state::{BatchPhase, BatchState};

use super::reconciliation::{ReconciliationResult, reconciliation_summary};

/// What to do next for one batch, given persisted phase and filesystem reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeStepPlan {
    CopyBatch,
    VerifyBatch,
    /// Verification passed; operator must approve (interactive or state flag) before delete.
    PendingDeleteApproval,
    DeleteSource,
    /// Delete finished; caller should run snapshot cadence logic if configured.
    PostDeleteSnapshot,
    BatchFullyCompleted,
    ConflictOperatorReview {
        reason: String,
    },
    BlockedFailedVerification,
}

/// Decide the next safe step for a batch. Does not mutate state.
pub fn plan_resume_step(
    batch_state: &BatchState,
    recon: &ReconciliationResult,
    batch: &Batch,
) -> ResumeStepPlan {
    plan_resume_step_with_recovery(batch_state, recon, batch, false)
}

pub fn plan_resume_step_with_recovery(
    batch_state: &BatchState,
    recon: &ReconciliationResult,
    _batch: &Batch,
    allow_failed_recovery: bool,
) -> ResumeStepPlan {
    match batch_state.phase {
        BatchPhase::Failed => match (
            allow_failed_recovery,
            recon.all_destination_files_ready,
            recon.size_mismatches.is_empty(),
        ) {
            (false, _, _) => ResumeStepPlan::ConflictOperatorReview {
                reason: format!(
                    "batch is marked failed; operator review required ({})",
                    reconciliation_summary(recon)
                ),
            },
            (true, true, _) => ResumeStepPlan::VerifyBatch,
            (true, false, false) => ResumeStepPlan::ConflictOperatorReview {
                reason: format!(
                    "failed-batch recovery blocked due to destination size mismatches ({})",
                    reconciliation_summary(recon)
                ),
            },
            (true, false, true) => ResumeStepPlan::CopyBatch,
        },
        BatchPhase::Planned => ResumeStepPlan::CopyBatch,
        BatchPhase::CopyStarted => {
            if recon.all_destination_files_ready {
                ResumeStepPlan::VerifyBatch
            } else {
                ResumeStepPlan::CopyBatch
            }
        }
        BatchPhase::CopyCompleted => {
            if recon.all_destination_files_ready {
                ResumeStepPlan::VerifyBatch
            } else {
                ResumeStepPlan::ConflictOperatorReview {
                    reason: format!(
                        "state says copy completed but destination is incomplete: missing {:?}, size mismatches {:?}",
                        recon.missing_in_destination, recon.size_mismatches
                    ),
                }
            }
        }
        BatchPhase::VerifyCompleted => match (
            batch_state.verification_passed,
            batch_state.approved_for_delete,
            batch_state.deleted,
        ) {
            (false, _, _) => ResumeStepPlan::BlockedFailedVerification,
            (true, false, _) => ResumeStepPlan::PendingDeleteApproval,
            (true, true, true) => ResumeStepPlan::PostDeleteSnapshot,
            (true, true, false) => ResumeStepPlan::DeleteSource,
        },
        BatchPhase::ApprovedForDelete => {
            if !batch_state.verification_passed {
                ResumeStepPlan::BlockedFailedVerification
            } else if batch_state.deleted {
                ResumeStepPlan::PostDeleteSnapshot
            } else {
                ResumeStepPlan::DeleteSource
            }
        }
        BatchPhase::DeleteCompleted => ResumeStepPlan::PostDeleteSnapshot,
        BatchPhase::SnapshotCompleted => ResumeStepPlan::BatchFullyCompleted,
    }
}
