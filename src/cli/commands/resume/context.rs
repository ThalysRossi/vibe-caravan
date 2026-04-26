use std::path::{Path, PathBuf};

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::signal::{ShutdownFlag, check_shutdown};
use crate::{snapshot, state_store, transfer};

pub(super) struct ResumeContext<'a> {
    pub(super) config: &'a TransferConfig,
    pub(super) state_path: PathBuf,
    pub(super) shutdown_flag: ShutdownFlag,
    pub(super) copy_backend: transfer::LocalFsCopyBackend,
    pub(super) snapshot_backend: snapshot::SystemSnapshotBackend,
}

impl<'a> ResumeContext<'a> {
    pub(super) fn new(
        config: &'a TransferConfig,
        state_path: &Path,
        shutdown_flag: ShutdownFlag,
    ) -> Self {
        Self {
            config,
            state_path: state_path.to_path_buf(),
            shutdown_flag,
            copy_backend: transfer::LocalFsCopyBackend::with_transfer_config(config),
            snapshot_backend: snapshot::SystemSnapshotBackend,
        }
    }

    pub(super) fn check_shutdown(&self) -> Result<(), CaravanError> {
        check_shutdown(&self.shutdown_flag)
    }

    pub(super) fn persist_state(&self, state: &MigrationState) -> Result<(), CaravanError> {
        state_store::persist_state(&self.state_path, state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConflictPolicy, CopyStrategy, Mode};

    #[test]
    fn persist_state_writes_state_file() {
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

        let context = ResumeContext {
            config: &config,
            state_path: tmp.path().join("resume-state.json"),
            shutdown_flag: ShutdownFlag::new(),
            copy_backend: transfer::LocalFsCopyBackend::with_transfer_config(&config),
            snapshot_backend: snapshot::SystemSnapshotBackend,
        };
        let state = MigrationState::new("staging", "/src", "/dst");

        context.persist_state(&state).expect("state should persist");
        assert!(context.state_path.exists());
    }
}
