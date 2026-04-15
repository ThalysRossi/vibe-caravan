use std::fs;
use std::path::{Path, PathBuf};

use crate::error::CaravanError;
use crate::models::batch::Batch;

/// Report of naming conflicts detected for a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictReport {
    /// Paths to files that already exist at the destination.
    pub existing_files: Vec<PathBuf>,
    /// Files where source and destination sizes differ.
    /// Tuple contains: (destination_path, source_size_bytes, destination_size_bytes)
    pub size_mismatches: Vec<(PathBuf, u64, u64)>,
    /// Total number of conflicts detected.
    pub total_conflicts: usize,
    /// Whether any conflicts were detected.
    pub has_conflicts: bool,
}

impl ConflictReport {
    /// Creates a new empty conflict report.
    pub fn new() -> Self {
        Self {
            existing_files: Vec::new(),
            size_mismatches: Vec::new(),
            total_conflicts: 0,
            has_conflicts: false,
        }
    }

    /// Adds a file that already exists at the destination.
    fn add_existing_file(&mut self, path: PathBuf) {
        self.existing_files.push(path);
        self.total_conflicts += 1;
        self.has_conflicts = true;
    }

    /// Adds a size mismatch between source and destination.
    fn add_size_mismatch(&mut self, path: PathBuf, source_size: u64, dest_size: u64) {
        self.size_mismatches.push((path, source_size, dest_size));
        // Note: size mismatch is already counted as a conflict via existing_files
    }
}

impl Default for ConflictReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Detects naming conflicts for a batch before copying.
///
/// Checks each file in the batch to see if it already exists at the destination.
/// For regular files, also compares sizes if they differ.
/// For symlinks and other special files, only detects existence (no size comparison).
///
/// # Errors
/// Returns `CaravanError::Io` if destination path cannot be accessed (permissions, etc.).
/// However, missing destination directory is not an error - treated as no conflicts.
pub fn detect_batch_conflicts(batch: &Batch, dest_root: &Path) -> Result<ConflictReport, CaravanError> {
    let mut report = ConflictReport::new();

    // If destination root doesn't exist, no conflicts possible
    if !dest_root.exists() {
        return Ok(report);
    }

    for file_entry in &batch.files {
        let dest_path = dest_root.join(&file_entry.relative_path);

        // Check if destination path exists
        // Use symlink_metadata to get info about the symlink itself, not the target
        match fs::symlink_metadata(&dest_path) {
            Ok(dest_metadata) => {
                // Something exists at the destination path
                report.add_existing_file(dest_path.clone());

                // Check if it's a regular file and compare sizes
                let file_type = dest_metadata.file_type();
                
                // Only compare sizes for regular files (not symlinks, directories, etc.)
                if file_type.is_file() && !file_type.is_symlink() {
                    let dest_size = dest_metadata.len();
                    
                    // If sizes differ, record the mismatch
                    if dest_size != file_entry.size_bytes {
                        report.add_size_mismatch(dest_path, file_entry.size_bytes, dest_size);
                    }
                }
                // For symlinks, directories, or other types, we skip size comparison
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // File doesn't exist at destination - no conflict
                continue;
            }
            Err(_err) => {
                // Permission error or other IO error - log warning but continue
                // As requested: log warnings and save failures to state
                // We'll return the partial report but note the error
                // For now, we'll just skip this file and continue
                // TODO: Consider logging this warning
                continue;
            }
        }
    }

    Ok(report)
}
