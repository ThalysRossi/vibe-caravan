use std::collections::HashSet;
use std::path::Path;

use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::progress::ProgressReporter;

/// Trait for directory creation abstraction, primarily for testing.
pub trait DirectoryCreator {
    /// Creates a directory and all of its parent directories if they are missing.
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()>;
}

/// Standard directory creator that uses the filesystem.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsDirectoryCreator;

impl DirectoryCreator for FsDirectoryCreator {
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(path)
    }
}

/// Trait for file copying abstraction, allowing different copy strategies.
pub trait FileCopier {
    /// Copy a single file from source to destination.
    /// Returns the number of bytes copied on success.
    fn copy_file(&self, source: &Path, destination: &Path) -> std::io::Result<u64>;
}

/// Simple file copier that uses the operating system's copy functionality.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsFileCopier;

impl FileCopier for OsFileCopier {
    fn copy_file(&self, source: &Path, destination: &Path) -> std::io::Result<u64> {
        std::fs::copy(source, destination)
    }
}

/// Buffered file copier that reads and writes files in chunks.
/// This can be more efficient for large files or cross-filesystem copies.
#[derive(Debug, Clone)]
pub struct BufferedFileCopier {
    /// Size of the buffer used for copying (in bytes)
    buffer_size: usize,
}

impl BufferedFileCopier {
    /// Creates a new buffered file copier with the specified buffer size.
    pub fn new(buffer_size: usize) -> Self {
        Self { buffer_size }
    }
    
    /// Default buffer size (8 MiB)
    pub const DEFAULT_BUFFER_SIZE: usize = 8 * 1024 * 1024;
    
    /// Creates a new buffered file copier with the default buffer size.
    pub fn default() -> Self {
        Self::new(Self::DEFAULT_BUFFER_SIZE)
    }
}

impl Default for BufferedFileCopier {
    fn default() -> Self {
        Self::new(Self::DEFAULT_BUFFER_SIZE)
    }
}

impl FileCopier for BufferedFileCopier {
    fn copy_file(&self, source: &Path, destination: &Path) -> std::io::Result<u64> {
        use std::io::{Read, Write};
        
        let mut source_file = std::fs::File::open(source)?;
        let mut dest_file = std::fs::File::create(destination)?;
        
        let mut buffer = vec![0u8; self.buffer_size];
        let mut total_copied = 0u64;
        
        loop {
            let bytes_read = source_file.read(&mut buffer)?;
            if bytes_read == 0 {
                break; // EOF
            }
            
            dest_file.write_all(&buffer[..bytes_read])?;
            total_copied += bytes_read as u64;
        }
        
        Ok(total_copied)
    }
}

/// Hybrid file copier that chooses between OS copy and buffered copy based on file size.
/// Files smaller than the threshold use OS copy, larger files use buffered copy.
#[derive(Debug, Clone)]
pub struct HybridFileCopier {
    /// Buffer size for buffered copy (in bytes)
    buffer_size: usize,
    /// File size threshold (in bytes) to use buffered copy instead of OS copy
    threshold: u64,
}

impl HybridFileCopier {
    /// Creates a new hybrid file copier with the specified buffer size and threshold.
    pub fn new(buffer_size: usize, threshold: u64) -> Self {
        Self { buffer_size, threshold }
    }
    
    /// Creates a new hybrid file copier with default values.
    /// - Buffer size: 8 MiB (8 * 1024 * 1024)
    /// - Threshold: 1 MiB (1 * 1024 * 1024)
    pub fn with_defaults() -> Self {
        Self::new(
            BufferedFileCopier::DEFAULT_BUFFER_SIZE,
            1 * 1024 * 1024, // 1 MiB
        )
    }
}

impl Default for HybridFileCopier {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl FileCopier for HybridFileCopier {
    fn copy_file(&self, source: &Path, destination: &Path) -> std::io::Result<u64> {
        // Get file size to decide which copier to use
        let metadata = std::fs::metadata(source)?;
        let file_size = metadata.len();
        
        if file_size < self.threshold {
            // Use OS copy for small files
            OsFileCopier.copy_file(source, destination)
        } else {
            // Use buffered copy for large files
            let buffered_copier = BufferedFileCopier::new(self.buffer_size);
            buffered_copier.copy_file(source, destination)
        }
    }
}

pub trait CopyBackend {
    fn copy_batch(
        &self,
        batch: &Batch,
        source_root: &Path,
        destination_root: &Path,
    ) -> Result<(), CaravanError>;
    
    fn copy_batch_with_progress(
        &self,
        batch: &Batch,
        source_root: &Path,
        destination_root: &Path,
        progress: &mut dyn ProgressReporter,
    ) -> Result<(), CaravanError>;
}

#[derive(Debug, Clone)]
pub struct LocalFsCopyBackend {
    file_copier: HybridFileCopier,
}

impl LocalFsCopyBackend {
    /// Creates a new LocalFsCopyBackend with default file copier settings.
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Creates a new LocalFsCopyBackend with custom buffer size and threshold.
    pub fn with_config(buffer_size: usize, threshold: u64) -> Self {
        Self {
            file_copier: HybridFileCopier::new(buffer_size, threshold),
        }
    }
}

impl Default for LocalFsCopyBackend {
    fn default() -> Self {
        Self {
            file_copier: HybridFileCopier::default(),
        }
    }
}

impl CopyBackend for LocalFsCopyBackend {
    fn copy_batch(
        &self,
        batch: &Batch,
        source_root: &Path,
        destination_root: &Path,
    ) -> Result<(), CaravanError> {
        self.copy_batch_with_progress(batch, source_root, destination_root, &mut crate::progress::NoopProgress::default())
    }
    
    fn copy_batch_with_progress(
        &self,
        batch: &Batch,
        source_root: &Path,
        destination_root: &Path,
        progress: &mut dyn ProgressReporter,
    ) -> Result<(), CaravanError> {
        progress.start(batch.files.len(), "Copying");
        
        // Track directories we've already created to avoid redundant system calls
        let mut created_dirs = HashSet::new();
        let dir_creator = FsDirectoryCreator;
        
        for (index, file) in batch.files.iter().enumerate() {
            let source_path = source_root.join(&file.relative_path);
            let destination_path = destination_root.join(&file.relative_path);

            // Create parent directory if needed, with deduplication
            if let Some(parent) = destination_path.parent() {
                if !created_dirs.contains(parent) {
                    dir_creator.create_dir_all(parent).map_err(|err| {
                        CaravanError::InvalidArguments(format!(
                            "failed to create directory '{}' while processing file '{}': {err}",
                            parent.display(),
                            file.relative_path.display()
                        ))
                    })?;
                    created_dirs.insert(parent.to_path_buf());
                }
            }

            self.file_copier.copy_file(&source_path, &destination_path).map_err(|err| {
                CaravanError::InvalidArguments(format!(
                    "failed to copy {} to {}: {err}",
                    source_path.display(),
                    destination_path.display()
                ))
            })?;
            
            progress.advance(index + 1, Some(&file.relative_path.to_string_lossy()));
        }

        progress.finish();
        Ok(())
    }
}

pub fn transfer_batch(
    batch: &Batch,
    source_root: &Path,
    destination_root: &Path,
    backend: &dyn CopyBackend,
) -> Result<(), CaravanError> {
    transfer_batch_with_progress(batch, source_root, destination_root, backend, &mut crate::progress::NoopProgress::default())
}

pub fn transfer_batch_with_progress(
    batch: &Batch,
    source_root: &Path,
    destination_root: &Path,
    backend: &dyn CopyBackend,
    progress: &mut dyn ProgressReporter,
) -> Result<(), CaravanError> {
    backend.copy_batch_with_progress(batch, source_root, destination_root, progress)
}
