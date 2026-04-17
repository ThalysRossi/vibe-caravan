use std::path::{Path, PathBuf};

use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::state_store::load_state;

pub fn detect_state_file(source: &Path, dest: &Path) -> Option<PathBuf> {
    let source_state = source.join(".caravan/state.json");
    if source_state.exists() {
        return Some(source_state);
    }

    let dest_state = dest.join(".caravan/state.json");
    if dest_state.exists() {
        return Some(dest_state);
    }

    let current_state = PathBuf::from(".caravan/state.json");
    if current_state.exists() {
        return Some(current_state);
    }

    None
}

pub fn check_state_file_compatibility(
    state_path: &Path,
    cli_batch_size: u64,
) -> Result<MigrationState, CaravanError> {
    let state = load_state(state_path)?;

    if state.batch_size_bytes == 0 || state.batch_size_bytes == cli_batch_size {
        return Ok(state);
    }

    Err(CaravanError::InvalidArguments(format!(
        "Batch size mismatch: state has {} bytes, CLI specifies {} bytes",
        state.batch_size_bytes, cli_batch_size
    )))
}
