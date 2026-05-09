use crate::error::CaravanError;
use crate::models::state::BatchState;

use super::failure::{FailureClass, recovery_message};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResumeOptions {
    pub interactive: bool,
    /// Scripting escape hatch: explicit operator consent without persisting approval in state yet.
    pub explicit_delete_approval: bool,
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
