use std::fs;
use std::path::Path;

use crate::atomic_write;
use crate::error::CaravanError;
use crate::models::state::MigrationState;

pub fn persist_state(path: &Path, state: &MigrationState) -> Result<(), CaravanError> {
    let payload = serde_json::to_string_pretty(state).map_err(|err| {
        CaravanError::InvalidArguments(format!("failed to serialize state: {err}"))
    })?;

    atomic_write::write_bytes(path, payload.as_bytes(), "state")
}

/// Persist canonical state and best-effort compatibility backup.
///
/// Canonical state write is mandatory; compatibility backup failures are
/// downgraded to warnings so progress isn't lost due to backup path issues.
pub fn persist_state_with_compat_backup(
    primary_path: &Path,
    secondary_path: &Path,
    state: &MigrationState,
) -> Result<(), CaravanError> {
    persist_state(primary_path, state)?;
    if let Err(err) = persist_state(secondary_path, state) {
        eprintln!(
            "[WARNING] failed to update compatibility backup state at {}: {}. Canonical state at {} remains authoritative.",
            secondary_path.display(),
            err,
            primary_path.display()
        );
    }
    Ok(())
}

pub fn load_state(path: &Path) -> Result<MigrationState, CaravanError> {
    let payload = fs::read_to_string(path).map_err(|source| CaravanError::StateRead {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str::<MigrationState>(&payload).map_err(|source| CaravanError::StateParse {
        path: path.to_path_buf(),
        source,
    })
}
