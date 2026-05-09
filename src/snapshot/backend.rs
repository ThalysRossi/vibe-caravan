use std::path::Path;

#[cfg(target_os = "linux")]
use super::platform_linux::create_btrfs_snapshot;
#[cfg(target_os = "windows")]
use super::platform_windows::create_btrfs_snapshot;
use crate::error::CaravanError;

pub trait SnapshotBackend {
    fn create_snapshot(
        &self,
        destination_root: &Path,
        snapshot_root: Option<&Path>,
        batch_id: &str,
    ) -> Result<String, CaravanError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemSnapshotBackend;

impl SnapshotBackend for SystemSnapshotBackend {
    fn create_snapshot(
        &self,
        destination_root: &Path,
        snapshot_root: Option<&Path>,
        batch_id: &str,
    ) -> Result<String, CaravanError> {
        create_btrfs_snapshot(destination_root, snapshot_root, batch_id)
    }
}
