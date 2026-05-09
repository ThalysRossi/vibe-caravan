use std::path::Path;

use crate::error::CaravanError;

pub(super) fn create_btrfs_snapshot(
    _destination_root: &Path,
    _snapshot_root: Option<&Path>,
    _batch_id: &str,
) -> Result<String, CaravanError> {
    Err(CaravanError::InvalidArguments(
        "btrfs snapshots are only supported on Linux".to_string(),
    ))
}
