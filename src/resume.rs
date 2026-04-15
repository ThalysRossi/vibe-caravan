use std::fs;
use std::path::Path;

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
        let msg = err.to_string();
        let class = if msg.contains("failed to read state file") {
            FailureClass::IoError
        } else {
            FailureClass::StateCorrupted
        };
        CaravanError::Resume {
            class: class.as_str().to_string(),
            detail: msg,
        }
    })
}

/// Decide the next safe step for a batch. Does not mutate state.
pub fn plan_resume_step(
    batch_state: &BatchState,
    recon: &ReconciliationResult,
    _batch: &Batch,
) -> ResumeStepPlan {
    match batch_state.phase {
        BatchPhase::Failed => ResumeStepPlan::ConflictOperatorReview {
            reason: "batch is marked failed; operator review required".to_string(),
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

/// Map common external failures into a stable class (for logging / UI).
pub fn classify_verification_failure_message(_detail: &str) -> FailureClass {
    FailureClass::VerificationMismatch
}

pub fn classify_copy_failure_message(_detail: &str) -> FailureClass {
    FailureClass::CopyBackendFailure
}

pub fn classify_capacity_failure_message(_detail: &str) -> FailureClass {
    FailureClass::CapacityExhausted
}

/// Entry point for CLI resume: load checkpoint from disk (same rules as [`load_state_for_resume`]).
pub fn resume_run(state_path: &Path) -> Result<MigrationState, CaravanError> {
    load_state_for_resume(state_path)
}
