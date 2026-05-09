use std::io;
use std::path::Path;

pub(super) fn system_native_copy_file(source: &Path, destination: &Path) -> io::Result<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::CopyFileW;

    let source_wide: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let copied = unsafe { CopyFileW(source_wide.as_ptr(), destination_wide.as_ptr(), 0) };
    if copied == 0 {
        return Err(io::Error::last_os_error());
    }

    std::fs::metadata(destination).map(|meta| meta.len())
}

pub(super) fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}
