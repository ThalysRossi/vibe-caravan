use std::collections::HashSet;
use std::path::Path;

use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::progress::ProgressReporter;

use super::atomic_copy::{copy_file_atomically, sync_parent_directories};
use super::copier::FileCopier;

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

pub fn copy_batch_with_components_and_durability(
    batch: &Batch,
    source_root: &Path,
    destination_root: &Path,
    file_copier: &dyn FileCopier,
    dir_creator: &dyn DirectoryCreator,
    progress: &mut dyn ProgressReporter,
    durable_writes: bool,
    check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
) -> Result<(), CaravanError> {
    progress.set_total_bytes(batch.total_bytes);
    progress.start(batch.files.len(), "Copying");

    let mut created_dirs = HashSet::new();
    let mut touched_parent_dirs = HashSet::new();

    for (index, file) in batch.files.iter().enumerate() {
        check_interrupt()?;

        let source_path = source_root.join(&file.relative_path);
        let destination_path = destination_root.join(&file.relative_path);

        if let Some(parent) = destination_path.parent() {
            if !created_dirs.contains(parent) {
                dir_creator
                    .create_dir_all(parent)
                    .map_err(|source| CaravanError::IoContext {
                        context: format!(
                            "failed to create directory '{}' while processing file '{}'",
                            parent.display(),
                            file.relative_path.display()
                        ),
                        source,
                    })?;
                created_dirs.insert(parent.to_path_buf());
            }
        }

        copy_file_atomically(
            file_copier,
            &source_path,
            &destination_path,
            file.size_bytes,
            durable_writes,
        )?;

        if durable_writes {
            if let Some(parent) = destination_path.parent() {
                touched_parent_dirs.insert(parent.to_path_buf());
            }
        }

        progress.advance(index + 1, Some(&file.relative_path.to_string_lossy()));
    }

    if durable_writes {
        sync_parent_directories(touched_parent_dirs)?;
    }

    progress.finish();
    Ok(())
}
