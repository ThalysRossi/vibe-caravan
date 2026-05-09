use std::io;
use std::path::Path;

pub(super) fn system_native_copy_file(_source: &Path, _destination: &Path) -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native copy strategy is unavailable on this platform",
    ))
}

pub(super) fn sync_directory(path: &Path) -> io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}
