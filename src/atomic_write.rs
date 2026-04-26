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

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "windows")]
fn sync_parent_directory(_path: &Path, _subject: &str) -> Result<(), CaravanError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    use tempfile::tempdir;

    #[test]
    fn write_bytes_creates_parent_directories_and_writes_payload() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("nested/state.json");

        write_bytes(&path, br#"{"ok":true}"#, "state").expect("write should succeed");

        let payload = fs::read(&path).expect("state should exist");
        assert_eq!(payload, br#"{"ok":true}"#);
    }

    #[test]
    fn write_bytes_replaces_existing_payload() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("state.json");
        fs::write(&path, b"old").expect("seed old payload");

        write_bytes(&path, b"new", "state").expect("write should succeed");

        let payload = fs::read(&path).expect("state should exist");
        assert_eq!(payload, b"new");
    }

    #[test]
    fn write_bytes_rejects_path_without_filename() {
        let err = write_bytes(Path::new("/"), b"{}", "state").expect_err("must reject root path");
        let rendered = err.to_string();
        assert!(
            rendered.contains("state path must include a file name"),
            "unexpected error: {rendered}"
        );
    }

    #[test]
    fn temp_path_for_keeps_parent_and_appends_tmp_suffix() {
        let original = Path::new("/tmp/state.json");
        let temp_path = temp_path_for(original, OsStr::new("state.json"));

        assert_eq!(temp_path.parent(), original.parent());
        let file_name = temp_path
            .file_name()
            .expect("temp file name")
            .to_string_lossy();
        assert!(
            file_name.starts_with("state.json."),
            "unexpected temp name: {file_name}"
        );
        assert!(
            file_name.ends_with(".tmp"),
            "unexpected temp name: {file_name}"
        );
    }

    #[test]
    fn unique_suffix_is_non_empty_and_contains_separator() {
        let suffix = unique_suffix();
        assert!(!suffix.is_empty());
        assert!(suffix.contains('.'));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sync_parent_directory_returns_ok_when_path_has_no_parent() {
        sync_parent_directory(Path::new(""), "state").expect("no-parent path should be ignored");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sync_parent_directory_reports_io_context_for_missing_parent() {
        let temp = tempdir().expect("tempdir");
        let missing_parent = temp.path().join("missing");
        let target = missing_parent.join("state.json");

        let err = sync_parent_directory(&target, "state").expect_err("missing parent must fail");
        let rendered = err.to_string();
        assert!(
            rendered.contains("failed to open state parent directory"),
            "unexpected error: {rendered}"
        );
    }
}
