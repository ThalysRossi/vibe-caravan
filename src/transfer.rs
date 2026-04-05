use std::fs;
use std::path::Path;

use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::progress::ProgressReporter;

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
        
        for (index, file) in batch.files.iter().enumerate() {
            let source_path = source_root.join(&file.relative_path);
            let destination_path = destination_root.join(&file.relative_path);

            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    CaravanError::InvalidArguments(format!(
                        "failed to create destination directory {}: {err}",
                        parent.display()
                    ))
                })?;
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
