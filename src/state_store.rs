use std::fs;
use std::io::Write;
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

    let file_name = path.file_name().ok_or_else(|| {
        CaravanError::InvalidArguments(format!(
            "state path must include a file name: {}",
            path.display()
        ))
    })?;
    let temp_path = path.with_file_name(format!("{}.tmp", file_name.to_string_lossy()));

    let write_result = (|| -> Result<(), CaravanError> {
        let mut file = fs::File::create(&temp_path).map_err(|err| {
            CaravanError::InvalidArguments(format!(
                "failed to create temporary state file {}: {err}",
                temp_path.display()
            ))
        })?;
        file.write_all(payload.as_bytes()).map_err(|err| {
            CaravanError::InvalidArguments(format!(
                "failed to write temporary state file {}: {err}",
                temp_path.display()
            ))
        })?;
        file.sync_all().map_err(|err| {
            CaravanError::InvalidArguments(format!(
                "failed to flush temporary state file {}: {err}",
                temp_path.display()
            ))
        })?;
        drop(file);

        fs::rename(&temp_path, path).map_err(|err| {
            CaravanError::InvalidArguments(format!(
                "failed to replace state file {}: {err}",
                path.display()
            ))
        })?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result?;

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
