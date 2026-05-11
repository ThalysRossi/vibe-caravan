use std::path::{Path, PathBuf};

use crate::error::CaravanError;

#[cfg(any(target_os = "linux", test))]
pub(super) fn resolve_existing_path(path: &Path) -> Result<PathBuf, CaravanError> {
    let mut candidate: Option<&Path> = Some(path);
    while let Some(current) = candidate {
        if current.exists() {
            return Ok(current.to_path_buf());
        }
        candidate = current.parent();
    }

    Err(CaravanError::InvalidArguments(format!(
        "path '{}' and its parents do not exist",
        path.display()
    )))
}

pub(super) fn path_within_or_equal(candidate: &Path, ancestor: &Path) -> bool {
    candidate == ancestor || candidate.starts_with(ancestor)
}

pub(super) fn canonical_path(path: &Path) -> Result<PathBuf, CaravanError> {
    std::fs::canonicalize(path).map_err(|err| {
        CaravanError::InvalidArguments(format!(
            "path '{}' is not accessible: {}",
            path.display(),
            err
        ))
    })
}

pub(super) fn canonical_path_for_maybe_missing(path: &Path) -> Result<PathBuf, CaravanError> {
    if path.exists() {
        return canonical_path(path);
    }

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|err| {
                CaravanError::InvalidArguments(format!(
                    "failed to read current directory while resolving '{}': {}",
                    path.display(),
                    err
                ))
            })?
            .join(path)
    };
    Ok(absolute)
}
