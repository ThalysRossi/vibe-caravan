use std::path::PathBuf;

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::signal::{ShutdownFlag, install_signal_handlers};
use crate::{migration_registry, snapshot, state_store, transfer};

pub(super) struct TransferContext<'a> {
    pub(super) config: &'a TransferConfig,
    pub(super) state_path: PathBuf,
    pub(super) secondary_state_path: PathBuf,
    pub(super) shutdown_flag: ShutdownFlag,
    pub(super) copy_backend: transfer::LocalFsCopyBackend,
    pub(super) snapshot_backend: snapshot::SystemSnapshotBackend,
}

impl<'a> TransferContext<'a> {
    pub(super) fn new(config: &'a TransferConfig) -> Result<Self, CaravanError> {
        let shutdown_flag = ShutdownFlag::new();
        install_signal_handlers(&shutdown_flag)?;

        let state_path = migration_registry::state_file_in_source(&config.source, &config.dest);
        let secondary_state_path = PathBuf::from(".caravan/state.json");
        let copy_backend = transfer::LocalFsCopyBackend::with_transfer_config(config);
        let snapshot_backend = snapshot::SystemSnapshotBackend;

        Ok(Self {
            config,
            state_path,
            secondary_state_path,
            shutdown_flag,
            copy_backend,
            snapshot_backend,
        })
    }

    pub(super) fn persist_state(&self, state: &MigrationState) -> Result<(), CaravanError> {
        state_store::persist_state_with_compat_backup(
            &self.state_path,
            &self.secondary_state_path,
            state,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConflictPolicy, CopyStrategy, Mode};

    #[test]
    fn persist_state_writes_primary_and_secondary_files() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let config = TransferConfig {
            mode: Mode::Staging,
            source: tmp.path().join("source"),
            dest: tmp.path().join("dest"),
            batch_size_bytes: 1024,
            max_files: None,
            snapshot_every: None,
            snapshot_dir: None,
            interactive: false,
            log_level: "info".to_string(),
            skip_conflicts: false,
            conflict_policy: ConflictPolicy::SkipFile,
            recover_failed: false,
            allow_unsafe_filesystems: false,
            copy_strategy: CopyStrategy::Auto,
        };
        std::fs::create_dir_all(&config.source).expect("create source");
        std::fs::create_dir_all(&config.dest).expect("create dest");

        let context = TransferContext {
            config: &config,
            state_path: tmp.path().join("primary-state.json"),
            secondary_state_path: tmp.path().join("secondary-state.json"),
            shutdown_flag: ShutdownFlag::new(),
            copy_backend: transfer::LocalFsCopyBackend::with_transfer_config(&config),
            snapshot_backend: snapshot::SystemSnapshotBackend,
        };
        let state = MigrationState::new("staging", "/src", "/dst");

        context.persist_state(&state).expect("state should persist");
        assert!(context.state_path.exists(), "primary state should exist");
        assert!(
            context.secondary_state_path.exists(),
            "secondary compatibility state should exist"
        );
    }
}
