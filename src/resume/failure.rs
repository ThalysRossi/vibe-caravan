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
