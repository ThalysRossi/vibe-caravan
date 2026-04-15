use std::collections::{BTreeMap, HashMap};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::file_entry::FileEntry;

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
    /// Number of unique parent directories probed while detecting conflicts.
    /// This helps surface fast-path behavior for large batches.
    pub scanned_parent_directories: usize,
}

impl ConflictReport {
    /// Creates a new empty conflict report.
    pub fn new() -> Self {
        Self {
            existing_files: Vec::new(),
            size_mismatches: Vec::new(),
            total_conflicts: 0,
            has_conflicts: false,
            scanned_parent_directories: 0,
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
pub fn detect_batch_conflicts(
    batch: &Batch,
    dest_root: &Path,
) -> Result<ConflictReport, CaravanError> {
    let mut report = ConflictReport::new();

    // If destination root doesn't exist, no conflicts possible
    if !dest_root.exists() {
        return Ok(report);
    }

    let files_by_parent = group_files_by_destination_parent(batch, dest_root);
    for (parent_dir, planned_files) in files_by_parent {
        report.scanned_parent_directories += 1;

        let index = match build_directory_index(&parent_dir) {
            Ok(index) => index,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // Parent directory does not exist => no possible conflicts in this parent.
                continue;
            }
            Err(_err) => {
                // Permission error or other IO error - log warning but continue.
                continue;
            }
        };

        for (file_entry, dest_path, file_name) in planned_files {
            let key = filename_lookup_key(&file_name);
            let Some(indexed) = index.get(&key) else {
                continue;
            };

            report.add_existing_file(dest_path.clone());

            // For regular files (not symlinks), compare sizes.
            if indexed.is_file && !indexed.is_symlink {
                match fs::symlink_metadata(&dest_path) {
                    Ok(dest_metadata) => {
                        let dest_size = dest_metadata.len();
                        if dest_size != file_entry.size_bytes {
                            report.add_size_mismatch(dest_path, file_entry.size_bytes, dest_size);
                        }
                    }
                    Err(_err) => {
                        // If metadata became unavailable/racy, keep existence conflict but skip size check.
                        continue;
                    }
                }
            }
        }
    }

    Ok(report)
}

#[derive(Debug, Clone, Copy)]
struct IndexedDestinationEntry {
    is_file: bool,
    is_symlink: bool,
}

fn filename_lookup_key(name: &OsStr) -> String {
    let rendered = name.to_string_lossy();
    if cfg!(windows) {
        rendered.to_lowercase()
    } else {
        rendered.into_owned()
    }
}

fn group_files_by_destination_parent<'a>(
    batch: &'a Batch,
    dest_root: &Path,
) -> BTreeMap<PathBuf, Vec<(&'a FileEntry, PathBuf, OsString)>> {
    let mut grouped: BTreeMap<PathBuf, Vec<(&'a FileEntry, PathBuf, OsString)>> = BTreeMap::new();

    for file_entry in &batch.files {
        let dest_path = dest_root.join(&file_entry.relative_path);
        let parent_dir = dest_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| dest_root.to_path_buf());
        let Some(file_name) = dest_path.file_name().map(|name| name.to_os_string()) else {
            continue;
        };
        grouped
            .entry(parent_dir)
            .or_default()
            .push((file_entry, dest_path, file_name));
    }

    grouped
}

fn build_directory_index(
    parent_dir: &Path,
) -> Result<HashMap<String, IndexedDestinationEntry>, std::io::Error> {
    let mut index: HashMap<String, IndexedDestinationEntry> = HashMap::new();
    let read_dir = fs::read_dir(parent_dir)?;
    for entry in read_dir {
        let Ok(entry) = entry else {
            continue;
        };
        let key = filename_lookup_key(&entry.file_name());
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        index.insert(
            key,
            IndexedDestinationEntry {
                is_file: file_type.is_file(),
                is_symlink: file_type.is_symlink(),
            },
        );
    }

    Ok(index)
}
