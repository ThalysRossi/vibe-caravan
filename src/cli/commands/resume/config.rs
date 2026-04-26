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

#[cfg(test)]
mod tests {
    use super::*;

    fn base_state(mode: &str) -> MigrationState {
        let mut state = MigrationState::new(mode, "/src", "/dst");
        state.batch_size_bytes = 1024;
        state.max_files = Some(7);
        state.snapshot_every = Some(3);
        state.snapshot_dir = Some("/snapshots".to_string());
        state
    }

    #[test]
    fn transfer_config_from_state_maps_common_fields() {
        let mut state = base_state("staging");
        state.copy_strategy = CopyStrategy::Auto;

        let config = transfer_config_from_state(&state, true).expect("config should be created");

        assert_eq!(config.mode, Mode::Staging);
        assert_eq!(config.source, PathBuf::from("/src"));
        assert_eq!(config.dest, PathBuf::from("/dst"));
        assert_eq!(config.batch_size_bytes, 1024);
        assert_eq!(config.max_files, Some(7));
        assert_eq!(config.snapshot_every, Some(3));
        assert_eq!(config.snapshot_dir, Some(PathBuf::from("/snapshots")));
        assert!(config.interactive);
        assert!(config.recover_failed);
    }

    #[test]
    fn transfer_config_from_state_rejects_unknown_mode() {
        let state = base_state("unexpected");
        let err = transfer_config_from_state(&state, false).expect_err("unknown mode must fail");
        assert!(err.to_string().contains("Unknown mode in state"));
    }

    #[test]
    fn normalize_legacy_buffered_strategy_falls_back_to_auto() {
        let normalized = normalize_legacy_copy_strategy(CopyStrategy::Buffered);
        assert_eq!(normalized, CopyStrategy::Auto);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn normalize_legacy_native_strategy_falls_back_to_auto_on_linux() {
        let normalized = normalize_legacy_copy_strategy(CopyStrategy::Native);
        assert_eq!(normalized, CopyStrategy::Auto);
    }
}
