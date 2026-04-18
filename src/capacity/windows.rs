use std::path::Path;

use crate::error::CaravanError;

use super::SpaceInfo;

pub(super) fn system_space_probe_backend() -> &'static str {
    "win32_getdiskfreespaceexw"
}

pub(super) fn query_space_info(destination: &Path) -> Result<SpaceInfo, CaravanError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide_path: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut free_bytes_available = 0u64;
    let mut total_number_of_bytes = 0u64;
    let mut total_number_of_free_bytes = 0u64;

    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide_path.as_ptr(),
            &mut free_bytes_available,
            &mut total_number_of_bytes,
            &mut total_number_of_free_bytes,
        )
    };

    if ok == 0 {
        return Err(CaravanError::IoContext {
            context: format!(
                "failed to read destination free space at {}",
                destination.display()
            ),
            source: std::io::Error::last_os_error(),
        });
    }

    Ok(SpaceInfo {
        total_bytes: total_number_of_bytes,
        available_bytes: free_bytes_available,
        volume_free_bytes: total_number_of_free_bytes,
    })
}

pub(super) fn destination_volume_root(destination: &Path) -> String {
    use std::path::{Component, Prefix};

    let mut components = destination.components();
    if let Some(Component::Prefix(prefix_component)) = components.next() {
        let prefix = prefix_component.kind();
        let root = match prefix {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                format!("{}:\\", (letter as char).to_ascii_uppercase())
            }
            _ => prefix_component.as_os_str().to_string_lossy().to_string(),
        };
        return root;
    }

    if destination.is_absolute() {
        "\\".to_string()
    } else {
        ".".to_string()
    }
}
