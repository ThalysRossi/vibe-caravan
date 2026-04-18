use std::path::Path;

use crate::error::CaravanError;

use super::SpaceInfo;

pub(super) fn system_space_probe_backend() -> &'static str {
    "fs2"
}

pub(super) fn query_space_info(destination: &Path) -> Result<SpaceInfo, CaravanError> {
    let total_bytes = fs2::total_space(destination).map_err(|source| CaravanError::IoContext {
        context: format!(
            "failed to read destination total capacity at {}",
            destination.display()
        ),
        source,
    })?;
    let available_bytes =
        fs2::available_space(destination).map_err(|source| CaravanError::IoContext {
            context: format!(
                "failed to read destination free space at {}",
                destination.display()
            ),
            source,
        })?;
    let volume_free_bytes = match fs2::free_space(destination) {
        Ok(bytes) => bytes,
        Err(_) => available_bytes,
    };

    Ok(SpaceInfo {
        total_bytes,
        available_bytes,
        volume_free_bytes,
    })
}

pub(super) fn destination_volume_root(destination: &Path) -> String {
    if destination.is_absolute() {
        "/".to_string()
    } else {
        ".".to_string()
    }
}
