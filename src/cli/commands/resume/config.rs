use std::path::PathBuf;

use crate::cli::args::validate_copy_option_values;
use crate::config::{ConflictPolicy, Mode, TransferConfig};
use crate::error::CaravanError;
use crate::models::state::MigrationState;

pub(super) fn transfer_config_from_state(
    state: &MigrationState,
    recover_failed: bool,
) -> Result<TransferConfig, CaravanError> {
    validate_copy_option_values(state.copy_buffer_size, state.buffered_copy_threshold)?;

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
        snapshot_dir: state.snapshot_dir.as_ref().map(PathBuf::from),
        interactive: true,
        log_level: "info".to_string(),
        skip_conflicts: false,
        conflict_policy: ConflictPolicy::SkipFile,
        recover_failed,
        allow_unsafe_filesystems: false,
        copy_strategy: state.copy_strategy,
        copy_buffer_size: state.copy_buffer_size,
        buffered_copy_threshold: state.buffered_copy_threshold,
    })
}
