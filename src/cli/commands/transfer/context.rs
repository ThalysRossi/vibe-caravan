use std::path::PathBuf;

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::signal::{install_signal_handlers, ShutdownFlag};
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
