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
