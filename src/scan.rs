use std::fs;
use std::path::{Path, PathBuf};

use crate::error::CaravanError;
use crate::models::file_entry::FileEntry;

pub fn scan_source(source_root: &Path) -> Result<Vec<FileEntry>, CaravanError> {
    if !source_root.exists() {
        return Err(CaravanError::InvalidArguments(format!(
            "source path does not exist: {}",
            source_root.display()
        )));
    }
    if !source_root.is_dir() {
        return Err(CaravanError::InvalidArguments(format!(
            "source path is not a directory: {}",
            source_root.display()
        )));
    }

    let mut entries = Vec::new();
    visit_dir(source_root, source_root, &mut entries)?;

    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(entries)
}

fn visit_dir(
    source_root: &Path,
    current_dir: &Path,
    output: &mut Vec<FileEntry>,
) -> Result<(), CaravanError> {
    let read_dir = fs::read_dir(current_dir).map_err(map_io("failed to read source directory"))?;
    let mut children: Vec<PathBuf> = read_dir
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_io("failed to enumerate source directory entries"))?;
    children.sort();

    for child in children {
        let metadata = fs::symlink_metadata(&child)
            .map_err(map_io("failed to read source file metadata"))?;
        if metadata.is_dir() {
            // Skip .caravan directories at any depth
            if let Some(file_name) = child.file_name() {
                if file_name == ".caravan" {
                    continue;
                }
            }
            visit_dir(source_root, &child, output)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }

        let relative_path = child.strip_prefix(source_root).map_err(|_| {
            CaravanError::InvalidArguments(
                "failed to derive relative path during scan".to_string(),
            )
        })?;
        output.push(FileEntry {
            relative_path: relative_path.to_path_buf(),
            size_bytes: metadata.len(),
            modified_time: metadata.modified().ok(),
        });
    }

    Ok(())
}

fn map_io(context: &'static str) -> impl Fn(std::io::Error) -> CaravanError {
    move |err| CaravanError::InvalidArguments(format!("{context}: {err}"))
}
