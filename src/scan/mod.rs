use std::fs;
use std::path::{Path, PathBuf};

use crate::error::CaravanError;
use crate::models::file_entry::FileEntry;
use crate::platform::is_windows_build;
use crate::progress::{NoopProgress, ProgressReporter};

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
    let mut progress = NoopProgress;
    scan_source_with_backend_and_progress(source_root, active_scan_backend(), &mut progress)
}

pub fn scan_source_with_progress(
    source_root: &Path,
    progress: &mut dyn ProgressReporter,
) -> Result<Vec<FileEntry>, CaravanError> {
    let mut no_interrupt = || Ok(());
    scan_source_with_backend_progress_and_interrupt(
        source_root,
        active_scan_backend(),
        progress,
        &mut no_interrupt,
    )
}

pub fn scan_source_with_progress_and_interrupt(
    source_root: &Path,
    progress: &mut dyn ProgressReporter,
    check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
) -> Result<Vec<FileEntry>, CaravanError> {
    scan_source_with_backend_progress_and_interrupt(
        source_root,
        active_scan_backend(),
        progress,
        check_interrupt,
    )
}

pub fn scan_source_with_backend(
    source_root: &Path,
    backend: ScanBackend,
) -> Result<Vec<FileEntry>, CaravanError> {
    let mut progress = NoopProgress;
    scan_source_with_backend_and_progress(source_root, backend, &mut progress)
}

pub fn scan_source_with_backend_and_progress(
    source_root: &Path,
    backend: ScanBackend,
    progress: &mut dyn ProgressReporter,
) -> Result<Vec<FileEntry>, CaravanError> {
    let mut no_interrupt = || Ok(());
    scan_source_with_backend_progress_and_interrupt(
        source_root,
        backend,
        progress,
        &mut no_interrupt,
    )
}

pub fn scan_source_with_backend_progress_and_interrupt(
    source_root: &Path,
    backend: ScanBackend,
    progress: &mut dyn ProgressReporter,
    check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
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
    check_interrupt()?;
    progress.start(0, "Scanning source");
    match backend {
        ScanBackend::StdFs => visit_dir_std(
            source_root,
            source_root,
            &mut entries,
            progress,
            check_interrupt,
        )?,
        ScanBackend::Win32FindFirstEx => visit_dir_win32(
            source_root,
            source_root,
            &mut entries,
            progress,
            check_interrupt,
        )?,
    }

    check_interrupt()?;
    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    progress.finish();
    Ok(entries)
}

pub(super) fn visit_dir_std(
    source_root: &Path,
    start_dir: &Path,
    output: &mut Vec<FileEntry>,
    progress: &mut dyn ProgressReporter,
    check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
) -> Result<(), CaravanError> {
    let mut stack: Vec<PathBuf> = Vec::new();
    push_children_in_reverse_sorted_order(start_dir, &mut stack, check_interrupt)?;

    while let Some(child) = stack.pop() {
        check_interrupt()?;
        let metadata =
            fs::symlink_metadata(&child).map_err(map_io("failed to read source file metadata"))?;
        if metadata.is_dir() {
            // Skip .caravan directories at any depth
            if matches!(child.file_name(), Some(file_name) if file_name == ".caravan") {
                continue;
            }
            push_children_in_reverse_sorted_order(&child, &mut stack, check_interrupt)?;
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
        let file_entry = FileEntry {
            relative_path: relative_path.to_path_buf(),
            size_bytes: metadata.len(),
            modified_time: metadata.modified().ok(),
        };
        output.push(file_entry);
        let item_name = output.last().map(|entry| entry.relative_path.as_path());
        progress.advance(output.len(), item_name.and_then(|path| path.to_str()));
        check_interrupt()?;
    }

    Ok(())
}

fn push_children_in_reverse_sorted_order(
    dir: &Path,
    stack: &mut Vec<PathBuf>,
    check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
) -> Result<(), CaravanError> {
    check_interrupt()?;
    let read_dir = fs::read_dir(dir).map_err(map_io("failed to read source directory"))?;
    let mut children: Vec<PathBuf> = Vec::new();
    for entry in read_dir {
        check_interrupt()?;
        let entry = entry.map_err(map_io("failed to enumerate source directory entries"))?;
        children.push(entry.path());
    }
    children.sort();
    for child in children.into_iter().rev() {
        check_interrupt()?;
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
