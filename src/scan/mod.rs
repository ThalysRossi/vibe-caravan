use std::fs;
use std::path::{Path, PathBuf};

use crate::error::CaravanError;
use crate::models::file_entry::FileEntry;
use crate::platform::is_windows_build;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux::visit_dir_win32;
#[cfg(target_os = "windows")]
use windows::visit_dir_win32;

#[cfg(target_os = "windows")]
pub use windows::windows_filetime_ticks_to_system_time;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanBackend {
    StdFs,
    Win32FindFirstEx,
}

pub const fn active_scan_backend() -> ScanBackend {
    if is_windows_build() {
        ScanBackend::Win32FindFirstEx
    } else {
        ScanBackend::StdFs
    }
}

pub fn scan_source(source_root: &Path) -> Result<Vec<FileEntry>, CaravanError> {
    scan_source_with_backend(source_root, active_scan_backend())
}

pub fn scan_source_with_backend(
    source_root: &Path,
    backend: ScanBackend,
) -> Result<Vec<FileEntry>, CaravanError> {
    scan_source_with_backend_impl(source_root, backend)
}

fn scan_source_with_backend_impl(
    source_root: &Path,
    backend: ScanBackend,
) -> Result<Vec<FileEntry>, CaravanError> {
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
    match backend {
        ScanBackend::StdFs => visit_dir_std(source_root, source_root, &mut entries)?,
        ScanBackend::Win32FindFirstEx => visit_dir_win32(source_root, source_root, &mut entries)?,
    }

    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(entries)
}

pub(super) fn visit_dir_std(
    source_root: &Path,
    start_dir: &Path,
    output: &mut Vec<FileEntry>,
) -> Result<(), CaravanError> {
    let mut stack: Vec<PathBuf> = Vec::new();
    push_children_in_reverse_sorted_order(start_dir, &mut stack)?;

    while let Some(child) = stack.pop() {
        let metadata =
            fs::symlink_metadata(&child).map_err(map_io("failed to read source file metadata"))?;
        if metadata.is_dir() {
            // Skip .caravan directories at any depth
            if matches!(child.file_name(), Some(file_name) if file_name == ".caravan") {
                continue;
            }
            push_children_in_reverse_sorted_order(&child, &mut stack)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }

        let relative_path = child.strip_prefix(source_root).map_err(|_| {
            CaravanError::StateCorrupt(format!(
                "failed to derive relative path during scan: {} is not under {}",
                child.display(),
                source_root.display()
            ))
        })?;
        output.push(FileEntry {
            relative_path: relative_path.to_path_buf(),
            size_bytes: metadata.len(),
            modified_time: metadata.modified().ok(),
        });
    }

    Ok(())
}

fn push_children_in_reverse_sorted_order(
    dir: &Path,
    stack: &mut Vec<PathBuf>,
) -> Result<(), CaravanError> {
    let read_dir = fs::read_dir(dir).map_err(map_io("failed to read source directory"))?;
    let mut children: Vec<PathBuf> = read_dir
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_io("failed to enumerate source directory entries"))?;
    children.sort();
    for child in children.into_iter().rev() {
        stack.push(child);
    }
    Ok(())
}

pub(super) fn map_io(context: &'static str) -> impl Fn(std::io::Error) -> CaravanError {
    move |source| CaravanError::IoContext {
        context: context.to_string(),
        source,
    }
}
