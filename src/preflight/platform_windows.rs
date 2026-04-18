use std::path::Path;

use crate::error::CaravanError;

use super::DestinationFlags;

pub(super) fn query_destination_flags(
    destination: &Path,
) -> Result<DestinationFlags, CaravanError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_REPARSE_POINT, GetFileAttributesW,
        INVALID_FILE_ATTRIBUTES,
    };

    let wide_path: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let attrs = unsafe { GetFileAttributesW(wide_path.as_ptr()) };
    if attrs == INVALID_FILE_ATTRIBUTES {
        let last_error = std::io::Error::last_os_error();
        if last_error.kind() == std::io::ErrorKind::NotFound {
            return Ok(DestinationFlags {
                is_compressed: false,
                is_reparse_point: false,
            });
        }

        return Err(CaravanError::IoContext {
            context: format!(
                "failed to inspect destination attributes at {}",
                destination.display()
            ),
            source: last_error,
        });
    }

    Ok(DestinationFlags {
        is_compressed: attrs & FILE_ATTRIBUTE_COMPRESSED != 0,
        is_reparse_point: attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0,
    })
}

pub(super) fn detect_filesystem_type(_path: &Path) -> Result<Option<String>, CaravanError> {
    Ok(None)
}
