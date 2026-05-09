use std::path::Path;

use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::state_store;

use super::failure::FailureClass;

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
