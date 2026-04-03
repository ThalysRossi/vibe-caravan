use std::fs;
use std::path::Path;

use crate::error::CaravanError;
use crate::models::state::MigrationState;

pub fn persist_state(path: &Path, state: &MigrationState) -> Result<(), CaravanError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            CaravanError::InvalidArguments(format!(
                "failed to create state directory {}: {err}",
                parent.display()
            ))
        })?;
    }

    let payload = serde_json::to_string_pretty(state)
        .map_err(|err| CaravanError::InvalidArguments(format!("failed to serialize state: {err}")))?;
    fs::write(path, payload).map_err(|err| {
        CaravanError::InvalidArguments(format!("failed to write state file {}: {err}", path.display()))
    })?;
    Ok(())
}

pub fn load_state(path: &Path) -> Result<MigrationState, CaravanError> {
    let payload = fs::read_to_string(path).map_err(|err| {
        CaravanError::InvalidArguments(format!("failed to read state file {}: {err}", path.display()))
    })?;
    serde_json::from_str::<MigrationState>(&payload).map_err(|err| {
        CaravanError::InvalidArguments(format!(
            "failed to parse state file {}: {err}",
            path.display()
        ))
    })
}
