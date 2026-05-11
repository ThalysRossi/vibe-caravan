use std::collections::HashSet;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
use super::platform_linux::sync_directory;
#[cfg(target_os = "windows")]
use super::platform_windows::sync_directory;
use crate::error::CaravanError;

use super::copier::FileCopier;

pub(super) fn copy_file_atomically(
    file_copier: &dyn FileCopier,
    source_path: &Path,
    destination_path: &Path,
    size_hint: u64,
    durable_writes: bool,
) -> Result<(), CaravanError> {
    let temp_destination_path = temp_destination_path(destination_path);
    clear_stale_temp_file(&temp_destination_path, destination_path)?;

    if let Err(source) =
        file_copier.copy_file_with_size_hint(source_path, &temp_destination_path, Some(size_hint))
    {
        let _ = std::fs::remove_file(&temp_destination_path);
        return Err(CaravanError::IoContext {
            context: format!(
                "failed to copy {} to {}",
                source_path.display(),
                destination_path.display()
            ),
            source,
        });
    }

    if durable_writes {
        if let Err(source) = sync_file_data(&temp_destination_path) {
            let _ = std::fs::remove_file(&temp_destination_path);
            return Err(CaravanError::IoContext {
                context: format!(
                    "failed to flush copied file before finalize {}",
                    destination_path.display()
                ),
                source,
            });
        }
    }

    if let Err(source) = std::fs::rename(&temp_destination_path, destination_path) {
        let _ = std::fs::remove_file(&temp_destination_path);
        return Err(CaravanError::IoContext {
            context: format!(
                "failed to finalize copied file {}",
                destination_path.display()
            ),
            source,
        });
    }

    Ok(())
}

pub(crate) fn temp_destination_path(destination_path: &Path) -> PathBuf {
    let file_name = destination_path
        .file_name()
        .map(|name| {
            let mut temp_name = OsString::from(name);
            temp_name.push(".caravan.part");
            temp_name
        })
        .unwrap_or_else(|| OsString::from(".caravan.part"));

    if let Some(parent) = destination_path.parent() {
        parent.join(file_name)
    } else {
        PathBuf::from(file_name)
    }
}

fn clear_stale_temp_file(temp_path: &Path, destination_path: &Path) -> Result<(), CaravanError> {
    match std::fs::remove_file(temp_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(CaravanError::IoContext {
            context: format!(
                "failed to remove stale temporary file for {} ({})",
                destination_path.display(),
                temp_path.display()
            ),
            source,
        }),
    }
}

fn sync_file_data(path: &Path) -> io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

pub(super) fn sync_parent_directories(parents: HashSet<PathBuf>) -> Result<(), CaravanError> {
    let mut parents: Vec<PathBuf> = parents.into_iter().collect();
    parents.sort();

    for parent in parents {
        sync_directory(&parent).map_err(|source| CaravanError::IoContext {
            context: format!("failed to flush destination directory {}", parent.display()),
            source,
        })?;
    }

    Ok(())
}
