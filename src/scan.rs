use std::fs;
use std::path::{Path, PathBuf};

use crate::error::CaravanError;
use crate::models::file_entry::FileEntry;

#[cfg(target_os = "windows")]
const WINDOWS_TO_UNIX_EPOCH_100NS: u64 = 116_444_736_000_000_000;

#[cfg(target_os = "windows")]
#[doc(hidden)]
pub fn windows_filetime_ticks_to_system_time(ticks_100ns: u64) -> Option<std::time::SystemTime> {
    use std::time::{Duration, UNIX_EPOCH};

    let unix_ticks = ticks_100ns.checked_sub(WINDOWS_TO_UNIX_EPOCH_100NS)?;
    let secs = unix_ticks / 10_000_000;
    let nanos = ((unix_ticks % 10_000_000) * 100) as u32;
    Some(UNIX_EPOCH + Duration::new(secs, nanos))
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
struct WinFindHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(target_os = "windows")]
impl WinFindHandle {
    fn try_new(raw: windows_sys::Win32::Foundation::HANDLE) -> Option<Self> {
        if raw == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            None
        } else {
            Some(Self(raw))
        }
    }

    fn as_raw(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.0
    }
}

#[cfg(target_os = "windows")]
impl Drop for WinFindHandle {
    fn drop(&mut self) {
        // SAFETY: `self.0` originates from a successful `FindFirstFileExW` call and this
        // guard enforces a single `FindClose` on scope exit.
        unsafe {
            let _ = windows_sys::Win32::Foundation::FindClose(self.0);
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
struct WinEnumeratedEntry {
    path: PathBuf,
    attributes: u32,
    size_bytes: u64,
    modified_time: Option<std::time::SystemTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanBackend {
    StdFs,
    Win32FindFirstEx,
}

pub const fn active_scan_backend() -> ScanBackend {
    if cfg!(target_os = "windows") {
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

fn visit_dir_std(
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

fn map_io(context: &'static str) -> impl Fn(std::io::Error) -> CaravanError {
    move |source| CaravanError::IoContext {
        context: context.to_string(),
        source,
    }
}

#[cfg(target_os = "windows")]
fn visit_dir_win32(
    source_root: &Path,
    start_dir: &Path,
    output: &mut Vec<FileEntry>,
) -> Result<(), CaravanError> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DEVICE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    };

    let mut stack: Vec<WinEnumeratedEntry> = Vec::new();
    push_win_children_in_reverse_sorted_order(start_dir, &mut stack)?;

    while let Some(child) = stack.pop() {
        let is_dir = child.attributes & FILE_ATTRIBUTE_DIRECTORY != 0;
        let is_reparse_point = child.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0;
        let is_device = child.attributes & FILE_ATTRIBUTE_DEVICE != 0;

        if is_reparse_point || is_device {
            continue;
        }

        if is_dir {
            if matches!(child.path.file_name(), Some(file_name) if file_name == ".caravan") {
                continue;
            }
            push_win_children_in_reverse_sorted_order(&child.path, &mut stack)?;
            continue;
        }

        let relative_path = child.path.strip_prefix(source_root).map_err(|_| {
            CaravanError::StateCorrupt(format!(
                "failed to derive relative path during scan: {} is not under {}",
                child.path.display(),
                source_root.display()
            ))
        })?;
        output.push(FileEntry {
            relative_path: relative_path.to_path_buf(),
            size_bytes: child.size_bytes,
            modified_time: child.modified_time,
        });
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn push_win_children_in_reverse_sorted_order(
    dir: &Path,
    stack: &mut Vec<WinEnumeratedEntry>,
) -> Result<(), CaravanError> {
    let children = enumerate_children_win32(dir)?;
    for child in children.into_iter().rev() {
        stack.push(child);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn enumerate_children_win32(current_dir: &Path) -> Result<Vec<WinEnumeratedEntry>, CaravanError> {
    use std::ffi::OsString;
    use std::mem;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::ptr;

    use windows_sys::Win32::Foundation::{ERROR_NO_MORE_FILES, GetLastError};
    use windows_sys::Win32::Storage::FileSystem::{
        FIND_FIRST_EX_LARGE_FETCH, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW,
        FindNextFileW, WIN32_FIND_DATAW,
    };

    fn wide_to_os_string(wide: &[u16]) -> OsString {
        let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
        OsString::from_wide(&wide[..len])
    }

    let search_pattern = current_dir.join("*");
    let search_wide: Vec<u16> = search_pattern
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: zero initialization matches Win32 expectations for output structs.
    let mut find_data: WIN32_FIND_DATAW = unsafe { mem::zeroed() };
    // SAFETY: pointers passed are valid for the duration of the call, and `find_data`
    // points to writable memory for the API to populate.
    let find_handle_raw = unsafe {
        FindFirstFileExW(
            search_wide.as_ptr(),
            FindExInfoBasic,
            &mut find_data as *mut _ as *mut _,
            FindExSearchNameMatch,
            ptr::null_mut(),
            FIND_FIRST_EX_LARGE_FETCH,
        )
    };

    let Some(find_handle) = WinFindHandle::try_new(find_handle_raw) else {
        return Err(map_io("failed to enumerate source directory entries")(
            std::io::Error::last_os_error(),
        ));
    };

    let mut children: Vec<WinEnumeratedEntry> = Vec::new();
    loop {
        let name = wide_to_os_string(&find_data.cFileName);
        let name_lossy = name.to_string_lossy();
        if name_lossy != "." && name_lossy != ".." {
            let size_bytes =
                ((find_data.nFileSizeHigh as u64) << 32) | find_data.nFileSizeLow as u64;
            let modified_ticks = ((find_data.ftLastWriteTime.dwHighDateTime as u64) << 32)
                | find_data.ftLastWriteTime.dwLowDateTime as u64;
            children.push(WinEnumeratedEntry {
                path: current_dir.join(&name),
                attributes: find_data.dwFileAttributes,
                size_bytes,
                modified_time: windows_filetime_ticks_to_system_time(modified_ticks),
            });
        }

        // SAFETY: `find_handle` is a valid search handle and `find_data` is writable.
        let has_next = unsafe { FindNextFileW(find_handle.as_raw(), &mut find_data) };
        if has_next == 0 {
            // SAFETY: reads thread-local Win32 last-error code after failed Win32 API call.
            let error_code = unsafe { GetLastError() };
            if error_code == ERROR_NO_MORE_FILES {
                break;
            }

            return Err(map_io("failed to enumerate source directory entries")(
                std::io::Error::from_raw_os_error(error_code as i32),
            ));
        }
    }

    children.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(children)
}

#[cfg(target_os = "linux")]
fn visit_dir_win32(
    source_root: &Path,
    start_dir: &Path,
    output: &mut Vec<FileEntry>,
) -> Result<(), CaravanError> {
    // Non-Windows fallback for tests and cross-platform behavior.
    visit_dir_std(source_root, start_dir, output)
}
