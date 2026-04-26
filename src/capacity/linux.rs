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

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    fn system_space_probe_backend_reports_fs2() {
        assert_eq!(system_space_probe_backend(), "fs2");
    }

    #[test]
    fn destination_volume_root_maps_absolute_and_relative_paths() {
        assert_eq!(destination_volume_root(Path::new("/tmp")), "/");
        assert_eq!(destination_volume_root(Path::new("relative/path")), ".");
    }

    #[test]
    fn query_space_info_returns_contextual_error_for_missing_destination() {
        let temp = tempdir().expect("tempdir");
        let missing = temp.path().join("does-not-exist");

        let err = query_space_info(&missing).expect_err("missing path must fail");
        let rendered = err.to_string();
        assert!(
            rendered.contains("failed to read destination total capacity"),
            "unexpected error: {rendered}"
        );
    }
}
