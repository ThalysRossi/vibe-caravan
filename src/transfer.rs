use std::collections::HashSet;
use std::fs;
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

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalFsCopyBackend;

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

            fs::copy(&source_path, &destination_path).map_err(|err| {
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
