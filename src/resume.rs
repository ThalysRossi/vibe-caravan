use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::state::{BatchPhase, BatchState, MigrationState};
use crate::state_store;

/// High-level failure categories for operator messaging and logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureClass {
    StateMissing,
    StateCorrupted,
    StateFilesystemConflict,
    CopyBackendFailure,
    VerificationMismatch,
    CapacityExhausted,
    IoError,
    ResumePolicyBlocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResumeStateSummary<'a> {
    pub mode: &'a str,
    pub source: &'a str,
    pub destination: &'a str,
    pub total_batches: usize,
    pub completed_batches: usize,
}

pub fn summarize_resume_state(state: &MigrationState) -> ResumeStateSummary<'_> {
    ResumeStateSummary {
        mode: &state.mode,
        source: &state.source,
        destination: &state.destination,
        total_batches: state.batches.len(),
        completed_batches: state.batches.iter().filter(|batch| batch.deleted).count(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResumeExecutionSummary {
    pub total_batches: usize,
    pub completed_batches: usize,
    pub pending_delete_batches: usize,
}

pub fn summarize_resume_execution(state: &MigrationState) -> ResumeExecutionSummary {
    ResumeExecutionSummary {
        total_batches: state.batches.len(),
        completed_batches: state.batches.iter().filter(|batch| batch.deleted).count(),
        pending_delete_batches: state.batches.iter().filter(|batch| !batch.deleted).count(),
    }
}

impl FailureClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            FailureClass::StateMissing => "state_missing",
            FailureClass::StateCorrupted => "state_corrupted",
            FailureClass::StateFilesystemConflict => "state_filesystem_conflict",
            FailureClass::CopyBackendFailure => "copy_backend_failure",
            FailureClass::VerificationMismatch => "verification_mismatch",
            FailureClass::CapacityExhausted => "capacity_exhausted",
            FailureClass::IoError => "io_error",
            FailureClass::ResumePolicyBlocked => "resume_policy_blocked",
        }
    }
}

/// Short, actionable text for operators (logs / stderr).
pub fn recovery_message(class: FailureClass) -> &'static str {
    match class {
        FailureClass::StateMissing => {
            "Cannot resume: state file is missing. Start a new run or restore state from backup."
        }
        FailureClass::StateCorrupted => {
            "Cannot resume: state file is unreadable or invalid JSON. Repair or restore state before continuing."
        }
        FailureClass::StateFilesystemConflict => {
            "Stop: persisted state disagrees with files on disk. Review partial copies or source changes before retrying."
        }
        FailureClass::CopyBackendFailure => {
            "Copy step failed. Source data should remain intact; fix the underlying error and retry the batch."
        }
        FailureClass::VerificationMismatch => {
            "Verification failed. Do not delete source data until you review mismatches and re-verify."
        }
        FailureClass::CapacityExhausted => {
            "Destination ran out of space or margin. Free space or reduce batch size before continuing."
        }
        FailureClass::IoError => {
            "An I/O error occurred. Check mounts, permissions, and hardware, then retry."
        }
        FailureClass::ResumePolicyBlocked => {
            "Resume blocked by safety policy: enable interactive approval or provide explicit delete approval before destructive steps."
        }
    }
}

/// Result of comparing the batch manifest to the destination tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationResult {
    pub all_destination_files_ready: bool,
    pub missing_in_destination: Vec<String>,
    pub size_mismatches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailedBatchInspection {
    pub batch_id: String,
    pub all_destination_files_ready: bool,
    pub missing_in_destination: Vec<String>,
    pub size_mismatches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailedBatchInspectionReport {
    pub failed_batch_count: usize,
    pub failed_batches: Vec<FailedBatchInspection>,
}

pub fn reconciliation_summary(recon: &ReconciliationResult) -> String {
    format!(
        "missing_in_destination={:?}; size_mismatches={:?}",
        recon.missing_in_destination, recon.size_mismatches
    )
}

/// For each file in the batch, check destination exists and size matches the planned entry.
pub fn reconcile_batch_destination(batch: &Batch, dest_root: &Path) -> ReconciliationResult {
    let mut missing_in_destination = Vec::new();
    let mut size_mismatches = Vec::new();

    for entry in &batch.files {
        let rel = entry.relative_path.to_string_lossy().to_string();
        let dest_path = dest_root.join(&entry.relative_path);
        match fs::metadata(&dest_path) {
            Ok(meta) => {
                if !meta.is_file() {
                    missing_in_destination.push(rel);
                } else if meta.len() != entry.size_bytes {
                    size_mismatches.push(rel);
                }
            }
            Err(_) => missing_in_destination.push(rel),
        }
    }

    let all_destination_files_ready =
        missing_in_destination.is_empty() && size_mismatches.is_empty();

    ReconciliationResult {
        all_destination_files_ready,
        missing_in_destination,
        size_mismatches,
    }
}

/// Build an inspection report for failed batches without mutating state.
pub fn inspect_failed_batches(
    state: &MigrationState,
    destination_root: &Path,
) -> Result<FailedBatchInspectionReport, CaravanError> {
    let failed_batch_ids: Vec<String> = state
        .batches
        .iter()
        .filter(|batch| batch.phase == BatchPhase::Failed && !batch.deleted)
        .map(|batch| batch.batch_id.clone())
        .collect();

    let mut failed_batches = Vec::with_capacity(failed_batch_ids.len());
    for batch_id in failed_batch_ids {
        let batch = state.materialize_planned_batch(&batch_id).ok_or_else(|| {
            CaravanError::StateCorrupt(format!(
                "missing immutable batch manifest for {}; cannot inspect failed batch safely",
                batch_id
            ))
        })?;
        let recon = reconcile_batch_destination(&batch, destination_root);
        failed_batches.push(FailedBatchInspection {
            batch_id,
            all_destination_files_ready: recon.all_destination_files_ready,
            missing_in_destination: recon.missing_in_destination,
            size_mismatches: recon.size_mismatches,
        });
    }

    Ok(FailedBatchInspectionReport {
        failed_batch_count: failed_batches.len(),
        failed_batches,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResumeOptions {
    pub interactive: bool,
    /// Scripting escape hatch: explicit operator consent without persisting approval in state yet.
    pub explicit_delete_approval: bool,
}

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

/// Load migration state for resume, with stable error classification.
pub fn load_state_for_resume(path: &Path) -> Result<MigrationState, CaravanError> {
    if !path.exists() {
        return Err(CaravanError::Resume {
            class: FailureClass::StateMissing.as_str().to_string(),
            detail: format!("state file does not exist: {}", path.display()),
        });
    }

    state_store::load_state(path).map_err(|err| {
        let class = match &err {
            CaravanError::StateRead { .. } => FailureClass::IoError,
            CaravanError::StateParse { .. } => FailureClass::StateCorrupted,
            _ => FailureClass::StateCorrupted,
        };
        CaravanError::Resume {
            class: class.as_str().to_string(),
            detail: err.to_string(),
        }
    })
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
        BatchPhase::Failed => {
            if !allow_failed_recovery {
                return ResumeStepPlan::ConflictOperatorReview {
                    reason: format!(
                        "batch is marked failed; operator review required ({})",
                        reconciliation_summary(recon)
                    ),
                };
            }

            if recon.all_destination_files_ready {
                ResumeStepPlan::VerifyBatch
            } else if !recon.size_mismatches.is_empty() {
                ResumeStepPlan::ConflictOperatorReview {
                    reason: format!(
                        "failed-batch recovery blocked due to destination size mismatches ({})",
                        reconciliation_summary(recon)
                    ),
                }
            } else {
                ResumeStepPlan::CopyBatch
            }
        }
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
        BatchPhase::VerifyCompleted => {
            if !batch_state.verification_passed {
                return ResumeStepPlan::BlockedFailedVerification;
            }
            if batch_state.approved_for_delete {
                if batch_state.deleted {
                    ResumeStepPlan::PostDeleteSnapshot
                } else {
                    ResumeStepPlan::DeleteSource
                }
            } else {
                ResumeStepPlan::PendingDeleteApproval
            }
        }
        BatchPhase::ApprovedForDelete => {
            if !batch_state.verification_passed {
                return ResumeStepPlan::BlockedFailedVerification;
            }
            if batch_state.deleted {
                ResumeStepPlan::PostDeleteSnapshot
            } else {
                ResumeStepPlan::DeleteSource
            }
        }
        BatchPhase::DeleteCompleted => ResumeStepPlan::PostDeleteSnapshot,
        BatchPhase::SnapshotCompleted => ResumeStepPlan::BatchFullyCompleted,
    }
}

/// Enforces fail-closed delete on resume when non-interactive and no explicit approval artifact.
pub fn require_delete_permission_for_resume(
    batch_state: &BatchState,
    opts: &ResumeOptions,
) -> Result<(), CaravanError> {
    if batch_state.deleted {
        return Ok(());
    }
    if !batch_state.verification_passed {
        return Err(CaravanError::Resume {
            class: FailureClass::ResumePolicyBlocked.as_str().to_string(),
            detail: "cannot delete: verification did not pass for this batch".to_string(),
        });
    }
    if batch_state.approved_for_delete || opts.explicit_delete_approval {
        return Ok(());
    }
    if opts.interactive {
        return Ok(());
    }
    Err(CaravanError::Resume {
        class: FailureClass::ResumePolicyBlocked.as_str().to_string(),
        detail: recovery_message(FailureClass::ResumePolicyBlocked).to_string(),
    })
}
