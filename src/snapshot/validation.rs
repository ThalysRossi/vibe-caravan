use std::path::Path;

use crate::config::Mode;
use crate::error::CaravanError;

use super::paths::{
    canonical_path, canonical_path_for_maybe_missing, path_within_or_equal, resolve_existing_path,
};

pub fn validate_snapshot_configuration(
    mode: Mode,
    snapshot_every: Option<u32>,
    destination_root: &Path,
    snapshot_root: Option<&Path>,
) -> Result<(), CaravanError> {
    if snapshot_root.is_some() && snapshot_every.is_none() {
        return Err(CaravanError::InvalidArguments(
            "snapshot-dir requires snapshot-every".to_string(),
        ));
    }

    if mode == Mode::Staging {
        if snapshot_every.is_some() || snapshot_root.is_some() {
            return Err(CaravanError::InvalidArguments(
                "snapshots are only supported in migrate mode".to_string(),
            ));
        }
        return Ok(());
    }

    if let Some(value) = snapshot_every {
        if value == 0 {
            return Err(CaravanError::InvalidArguments(
                "snapshot cadence must be greater than zero".to_string(),
            ));
        }
    } else {
        return Ok(());
    }

    let Some(snapshot_root) = snapshot_root else {
        return Ok(());
    };
    let snapshot_check_path = canonical_path(snapshot_root)?;
    let destination_check_path = canonical_path_for_maybe_missing(destination_root)?;
    if path_within_or_equal(&snapshot_check_path, &destination_check_path) {
        return Err(CaravanError::InvalidArguments(format!(
            "snapshot destination must not be inside migration destination: snapshot='{}', destination='{}'",
            snapshot_root.display(),
            destination_root.display()
        )));
    }

    let snapshot_meta = std::fs::metadata(snapshot_root).map_err(|err| {
        CaravanError::InvalidArguments(format!(
            "snapshot destination '{}' is not accessible: {}",
            snapshot_root.display(),
            err
        ))
    })?;
    if !snapshot_meta.is_dir() {
        return Err(CaravanError::InvalidArguments(format!(
            "snapshot destination must be an existing directory: '{}'",
            snapshot_root.display()
        )));
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let destination_probe = resolve_existing_path(destination_root)?;
        let destination_meta = std::fs::metadata(&destination_probe).map_err(|err| {
            CaravanError::InvalidArguments(format!(
                "destination '{}' is not accessible for snapshot checks: {}",
                destination_probe.display(),
                err
            ))
        })?;
        if snapshot_meta.dev() != destination_meta.dev() {
            return Err(CaravanError::InvalidArguments(format!(
                "snapshot destination '{}' must be on the same filesystem as destination '{}'",
                snapshot_root.display(),
                destination_root.display()
            )));
        }
    }

    Ok(())
}
