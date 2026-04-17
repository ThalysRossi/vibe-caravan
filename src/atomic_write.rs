use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::CaravanError;

pub fn write_bytes(path: &Path, payload: &[u8], subject: &str) -> Result<(), CaravanError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| CaravanError::IoContext {
            context: format!("failed to create {subject} directory {}", parent.display()),
            source,
        })?;
    }

    let file_name = path.file_name().ok_or_else(|| {
        CaravanError::InvalidArguments(format!(
            "{subject} path must include a file name: {}",
            path.display()
        ))
    })?;
    let temp_path = temp_path_for(path, file_name);

    let write_result = (|| -> Result<(), CaravanError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|source| CaravanError::IoContext {
                context: format!(
                    "failed to create temporary {subject} file {}",
                    temp_path.display()
                ),
                source,
            })?;
        file.write_all(payload)
            .map_err(|source| CaravanError::IoContext {
                context: format!(
                    "failed to write temporary {subject} file {}",
                    temp_path.display()
                ),
                source,
            })?;
        file.sync_all().map_err(|source| CaravanError::IoContext {
            context: format!(
                "failed to flush temporary {subject} file {}",
                temp_path.display()
            ),
            source,
        })?;
        drop(file);

        fs::rename(&temp_path, path).map_err(|source| CaravanError::IoContext {
            context: format!("failed to replace {subject} file {}", path.display()),
            source,
        })?;

        sync_parent_directory(path, subject)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

fn temp_path_for(path: &Path, file_name: &std::ffi::OsStr) -> PathBuf {
    let mut name = OsString::from(file_name);
    let unique = unique_suffix();
    name.push(format!(".{unique}.tmp"));
    path.with_file_name(name)
}

fn unique_suffix() -> String {
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}.{:x}", std::process::id(), now_nanos)
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path, subject: &str) -> Result<(), CaravanError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };

    let parent_dir = File::open(parent).map_err(|source| CaravanError::IoContext {
        context: format!(
            "failed to open {subject} parent directory {} for sync",
            parent.display()
        ),
        source,
    })?;
    parent_dir
        .sync_all()
        .map_err(|source| CaravanError::IoContext {
            context: format!(
                "failed to sync {subject} parent directory {}",
                parent.display()
            ),
            source,
        })?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path, _subject: &str) -> Result<(), CaravanError> {
    Ok(())
}
