use std::path::PathBuf;

use crate::config::{Mode, TransferConfig};
use crate::error::CaravanError;
use crate::models::state::MigrationState;

pub(super) fn transfer_config_from_state(
    state: &MigrationState,
) -> Result<TransferConfig, CaravanError> {
    Ok(TransferConfig {
        mode: match state.mode.as_str() {
            "staging" => Mode::Staging,
            "migrate" => Mode::Migrate,
            _ => {
                return Err(CaravanError::InvalidArguments(format!(
                    "Unknown mode in state: {}",
                    state.mode
                )));
            }
        },
        source: PathBuf::from(&state.source),
        dest: PathBuf::from(&state.destination),
        batch_size_bytes: state.batch_size_bytes,
        max_files: state.max_files,
        snapshot_every: state.snapshot_every,
        interactive: true,
        verification: state.verification_mode.clone(),
        log_level: "info".to_string(),
        skip_conflicts: false,
        copy_buffer_size: state.copy_buffer_size,
        buffered_copy_threshold: state.buffered_copy_threshold,
    })
}
