use std::path::Path;
use std::process::Command;

use crate::error::CaravanError;

use super::now_unix_secs;

pub(super) fn create_btrfs_snapshot(
    destination_root: &Path,
    snapshot_root: Option<&Path>,
    batch_id: &str,
) -> Result<String, CaravanError> {
    let snapshot_name = format!("caravan-snap-{}-{}", now_unix_secs(), batch_id);
    let snapshot_parent =
        snapshot_root.unwrap_or_else(|| destination_root.parent().unwrap_or(destination_root));
    let snapshot_path = snapshot_parent.join(&snapshot_name);

    let output = Command::new("btrfs")
        .args(["subvolume", "snapshot", "-r"])
        .arg(destination_root)
        .arg(&snapshot_path)
        .output()
        .map_err(|err| {
            CaravanError::InvalidArguments(format!(
                "failed to execute btrfs snapshot command for {}: {}",
                destination_root.display(),
                err
            ))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CaravanError::InvalidArguments(format!(
            "btrfs snapshot command failed for {} -> {}: {}",
            destination_root.display(),
            snapshot_path.display(),
            stderr.trim()
        )));
    }

    Ok(snapshot_name)
}
