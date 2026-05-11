use std::io;
use std::path::Path;

#[cfg(target_os = "linux")]
use super::platform_linux::system_native_copy_file;
#[cfg(target_os = "windows")]
use super::platform_windows::system_native_copy_file;

/// Trait for file copying abstraction, allowing different copy strategies.
pub trait FileCopier {
    /// Copy a single file from source to destination.
    /// Returns the number of bytes copied on success.
    fn copy_file(&self, source: &Path, destination: &Path) -> std::io::Result<u64>;

    /// Copy a single file with an optional caller-provided size hint in bytes.
    /// Default implementation falls back to `copy_file`.
    fn copy_file_with_size_hint(
        &self,
        source: &Path,
        destination: &Path,
        _size_hint: Option<u64>,
    ) -> std::io::Result<u64> {
        self.copy_file(source, destination)
    }
}

/// Simple file copier that uses the operating system's copy functionality.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsFileCopier;

impl FileCopier for OsFileCopier {
    fn copy_file(&self, source: &Path, destination: &Path) -> std::io::Result<u64> {
        std::fs::copy(source, destination)
    }
}

/// Native-first copier: tries OS-native copy API and falls back to OS copy on failure.
#[derive(Debug, Clone)]
pub struct NativePreferredFileCopier {
    fallback: OsFileCopier,
    native_copy: fn(&Path, &Path) -> io::Result<u64>,
}

impl NativePreferredFileCopier {
    pub fn new() -> Self {
        Self {
            fallback: OsFileCopier,
            native_copy: system_native_copy_file,
        }
    }
}

impl FileCopier for NativePreferredFileCopier {
    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<u64> {
        copy_with_native_preferred(
            source,
            destination,
            self.native_copy,
            |source, destination| self.fallback.copy_file(source, destination),
        )
    }
}

fn copy_with_native_preferred(
    source: &Path,
    destination: &Path,
    native_copy: impl FnOnce(&Path, &Path) -> io::Result<u64>,
    fallback_copy: impl FnOnce(&Path, &Path) -> io::Result<u64>,
) -> io::Result<u64> {
    let destination_preexisted = destination.exists();
    match native_copy(source, destination) {
        Ok(bytes) => Ok(bytes),
        Err(err) if is_storage_exhaustion_error(&err) => Err(err),
        Err(native_err) => {
            if !destination_preexisted {
                clear_partial_destination_before_fallback(destination, &native_err)?;
            }
            fallback_copy(source, destination)
        }
    }
}

fn clear_partial_destination_before_fallback(
    destination: &Path,
    native_err: &io::Error,
) -> io::Result<()> {
    match std::fs::remove_file(destination) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(io::Error::new(
            err.kind(),
            format!(
                "native copy failed ({native_err}); failed to remove partial destination before fallback {}: {err}",
                destination.display()
            ),
        )),
    }
}

fn is_storage_exhaustion_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded
    ) || matches!(
        err.raw_os_error(),
        Some(
            39   // ERROR_HANDLE_DISK_FULL
                | 112  // ERROR_DISK_FULL
                | 1295 // ERROR_DISK_QUOTA_EXCEEDED
                | 1816 // ERROR_NOT_ENOUGH_QUOTA
        )
    )
}

#[derive(Debug, Clone)]
pub(super) enum LocalFileCopier {
    Os(OsFileCopier),
    NativePreferred(NativePreferredFileCopier),
}

impl LocalFileCopier {
    fn copy_file_inner(&self, source: &Path, destination: &Path) -> io::Result<u64> {
        match self {
            LocalFileCopier::Os(copier) => copier.copy_file(source, destination),
            LocalFileCopier::NativePreferred(copier) => copier.copy_file(source, destination),
        }
    }
}

impl FileCopier for LocalFileCopier {
    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<u64> {
        self.copy_file_inner(source, destination)
    }

    fn copy_file_with_size_hint(
        &self,
        source: &Path,
        destination: &Path,
        _size_hint: Option<u64>,
    ) -> io::Result<u64> {
        self.copy_file_inner(source, destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tempfile::TempDir;

    #[test]
    fn native_preferred_does_not_fallback_when_native_copy_exhausts_storage() {
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("source.bin");
        let destination = temp.path().join("destination.bin");
        fs::write(&source, b"source").expect("source file");
        let fallback_called = AtomicBool::new(false);

        let err = copy_with_native_preferred(
            &source,
            &destination,
            |_source, destination| {
                fs::write(destination, b"partial").expect("partial native copy");
                Err(io::Error::from_raw_os_error(112))
            },
            |_source, _destination| {
                fallback_called.store(true, Ordering::SeqCst);
                Ok(0)
            },
        )
        .expect_err("storage exhaustion must stop without fallback");

        assert_eq!(err.raw_os_error(), Some(112));
        assert!(!fallback_called.load(Ordering::SeqCst));
        assert_eq!(
            fs::read(&destination).expect("partial file remains for atomic cleanup"),
            b"partial"
        );
    }

    #[test]
    fn native_preferred_removes_partial_destination_before_fallback() {
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("source.bin");
        let destination = temp.path().join("destination.bin");
        fs::write(&source, b"source").expect("source file");

        let bytes = copy_with_native_preferred(
            &source,
            &destination,
            |_source, destination| {
                fs::write(destination, b"partial").expect("partial native copy");
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "native copy unsupported",
                ))
            },
            |_source, destination| {
                assert!(
                    !destination.exists(),
                    "partial native destination should be removed before fallback"
                );
                fs::write(destination, b"fallback").expect("fallback copy");
                Ok(8)
            },
        )
        .expect("fallback should succeed");

        assert_eq!(bytes, 8);
        assert_eq!(
            fs::read(&destination).expect("fallback output"),
            b"fallback"
        );
    }

    #[test]
    fn native_preferred_does_not_remove_destination_that_preexisted_native_attempt() {
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("source.bin");
        let destination = temp.path().join("destination.bin");
        fs::write(&source, b"source").expect("source file");
        fs::write(&destination, b"existing").expect("existing destination");

        copy_with_native_preferred(
            &source,
            &destination,
            |_source, _destination| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "native copy unsupported",
                ))
            },
            |_source, destination| {
                assert!(
                    destination.exists(),
                    "preexisting destination should be left for fallback to handle"
                );
                fs::write(destination, b"fallback").expect("fallback copy");
                Ok(8)
            },
        )
        .expect("fallback should succeed");

        assert_eq!(
            fs::read(&destination).expect("fallback output"),
            b"fallback"
        );
    }

    #[test]
    fn storage_exhaustion_detection_covers_windows_raw_errors_and_rust_kinds() {
        for code in [39, 112, 1295, 1816] {
            assert!(is_storage_exhaustion_error(&io::Error::from_raw_os_error(
                code
            )));
        }
        assert!(is_storage_exhaustion_error(&io::Error::new(
            io::ErrorKind::StorageFull,
            "storage full"
        )));
        assert!(is_storage_exhaustion_error(&io::Error::new(
            io::ErrorKind::QuotaExceeded,
            "quota exceeded"
        )));
        assert!(!is_storage_exhaustion_error(&io::Error::new(
            io::ErrorKind::Unsupported,
            "unsupported"
        )));
    }
}
