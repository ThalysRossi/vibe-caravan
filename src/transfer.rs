use std::fs;
use std::path::Path;

use crate::error::WololoError;
use crate::models::batch::Batch;

pub trait CopyBackend {
    fn copy_batch(
        &self,
        batch: &Batch,
        source_root: &Path,
        destination_root: &Path,
    ) -> Result<(), WololoError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalFsCopyBackend;

impl CopyBackend for LocalFsCopyBackend {
    fn copy_batch(
        &self,
        batch: &Batch,
        source_root: &Path,
        destination_root: &Path,
    ) -> Result<(), WololoError> {
        for file in &batch.files {
            let source_path = source_root.join(&file.relative_path);
            let destination_path = destination_root.join(&file.relative_path);

            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    WololoError::InvalidArguments(format!(
                        "failed to create destination directory {}: {err}",
                        parent.display()
                    ))
                })?;
            }

            fs::copy(&source_path, &destination_path).map_err(|err| {
                WololoError::InvalidArguments(format!(
                    "failed to copy {} to {}: {err}",
                    source_path.display(),
                    destination_path.display()
                ))
            })?;
        }

        Ok(())
    }
}

pub fn transfer_batch(
    batch: &Batch,
    source_root: &Path,
    destination_root: &Path,
    backend: &dyn CopyBackend,
) -> Result<(), WololoError> {
    backend.copy_batch(batch, source_root, destination_root)
}
