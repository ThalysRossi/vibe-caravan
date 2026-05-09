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
}

impl NativePreferredFileCopier {
    pub fn new() -> Self {
        Self {
            fallback: OsFileCopier,
        }
    }
}

impl FileCopier for NativePreferredFileCopier {
    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<u64> {
        match system_native_copy_file(source, destination) {
            Ok(bytes) => Ok(bytes),
            Err(_) => self.fallback.copy_file(source, destination),
        }
    }
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
