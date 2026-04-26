use std::path::PathBuf;

use crate::config::{ConflictPolicy, CopyStrategy, Mode, TransferConfig};
use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::platform::is_windows_build;

pub(super) fn transfer_config_from_state(
    state: &MigrationState,
    recover_failed: bool,
) -> Result<TransferConfig, CaravanError> {
    let copy_strategy = normalize_legacy_copy_strategy(state.copy_strategy);

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
        copy_strategy,
    })
}

fn normalize_legacy_copy_strategy(strategy: CopyStrategy) -> CopyStrategy {
    match strategy {
        CopyStrategy::Auto => CopyStrategy::Auto,
        CopyStrategy::Buffered => {
            eprintln!(
                "[WARNING] state uses deprecated copy strategy 'buffered'; falling back to 'auto'."
            );
            CopyStrategy::Auto
        }
        CopyStrategy::Native if !is_windows_build() => {
            eprintln!(
                "[WARNING] state uses deprecated Linux copy strategy 'native'; falling back to 'auto'."
            );
            CopyStrategy::Auto
        }
        CopyStrategy::Native => CopyStrategy::Native,
    }
}
