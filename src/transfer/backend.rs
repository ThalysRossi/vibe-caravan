use std::path::Path;

use crate::config::{CopyStrategy, Mode, TransferConfig};
use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::progress::ProgressReporter;

use super::batch_copy::{FsDirectoryCreator, copy_batch_with_components_and_durability};
use super::copier::{LocalFileCopier, NativePreferredFileCopier, OsFileCopier};
use super::strategy::{ResolvedCopyStrategy, resolve_copy_strategy};

#[derive(Debug, Clone)]
pub struct LocalFsCopyBackend {
    file_copier: LocalFileCopier,
    durable_writes: bool,
}

impl LocalFsCopyBackend {
    /// Creates a new LocalFsCopyBackend with default file copier settings.
    pub fn new() -> Self {
        Self {
            file_copier: LocalFileCopier::Os(OsFileCopier),
            durable_writes: false,
        }
    }

    pub fn with_strategy(strategy: CopyStrategy, mode: &Mode) -> Self {
        let durable_writes = matches!(mode, Mode::Migrate);
        let file_copier = match resolve_copy_strategy(strategy, mode) {
            ResolvedCopyStrategy::Os => LocalFileCopier::Os(OsFileCopier),
            ResolvedCopyStrategy::NativePreferred => {
                LocalFileCopier::NativePreferred(NativePreferredFileCopier::new())
            }
        };
        Self {
            file_copier,
            durable_writes,
        }
    }

    pub fn with_transfer_config(config: &TransferConfig) -> Self {
        Self::with_strategy(config.copy_strategy, &config.mode)
    }

    pub fn durable_writes_enabled(&self) -> bool {
        self.durable_writes
    }

    pub fn copy_batch(
        &self,
        batch: &Batch,
        source_root: &Path,
        destination_root: &Path,
        progress: &mut dyn ProgressReporter,
        check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
    ) -> Result<(), CaravanError> {
        copy_batch_with_components_and_durability(
            batch,
            source_root,
            destination_root,
            &self.file_copier,
            &FsDirectoryCreator,
            progress,
            self.durable_writes,
            check_interrupt,
        )
    }
}

impl Default for LocalFsCopyBackend {
    fn default() -> Self {
        Self::new()
    }
}
