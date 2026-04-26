use std::collections::HashSet;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::{CopyStrategy, Mode, TransferConfig};
use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::state::MigrationState;
use crate::plan::PlanningSnapshot;
use crate::platform::is_windows_build;
use crate::progress::ProgressReporter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedCopyStrategy {
    Os,
    NativePreferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferPlanningSummary {
    pub batch_count: usize,
    pub source_file_count: usize,
    pub source_total_bytes: u64,
}

pub fn summarize_transfer_plan(plan: &PlanningSnapshot) -> TransferPlanningSummary {
    TransferPlanningSummary {
        batch_count: plan.batches.len(),
        source_file_count: plan.source_file_count,
        source_total_bytes: plan.source_total_bytes,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferExecutionSummary {
    pub processed_batches: u32,
    pub total_batches: usize,
    pub completed_batches: usize,
    pub pending_delete_batches: usize,
}

pub fn summarize_transfer_execution(
    state: &MigrationState,
    processed_batches: u32,
) -> TransferExecutionSummary {
    let total_batches = state.batches.len();
    let completed_batches = state.batches.iter().filter(|b| b.deleted).count();
    let pending_delete_batches = state.batches.iter().filter(|b| !b.deleted).count();

    TransferExecutionSummary {
        processed_batches,
        total_batches,
        completed_batches,
        pending_delete_batches,
    }
}

pub fn resolve_copy_strategy(strategy: CopyStrategy, mode: &Mode) -> ResolvedCopyStrategy {
    match strategy {
        CopyStrategy::Buffered => ResolvedCopyStrategy::Os,
        CopyStrategy::Native => {
            if is_windows_build() {
                ResolvedCopyStrategy::NativePreferred
            } else {
                ResolvedCopyStrategy::Os
            }
        }
        CopyStrategy::Auto => {
            if is_windows_build() && matches!(mode, Mode::Staging) {
                ResolvedCopyStrategy::NativePreferred
            } else {
                ResolvedCopyStrategy::Os
            }
        }
    }
}

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

#[cfg(target_os = "windows")]
fn system_native_copy_file(source: &Path, destination: &Path) -> io::Result<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::CopyFileW;

    let source_wide: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let copied = unsafe { CopyFileW(source_wide.as_ptr(), destination_wide.as_ptr(), 0) };
    if copied == 0 {
        return Err(io::Error::last_os_error());
    }

    std::fs::metadata(destination).map(|meta| meta.len())
}

#[cfg(target_os = "linux")]
fn system_native_copy_file(_source: &Path, _destination: &Path) -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native copy strategy is unavailable on this platform",
    ))
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
enum LocalFileCopier {
    Os(OsFileCopier),
    NativePreferred(NativePreferredFileCopier),
}

#[derive(Debug, Clone)]
pub struct LocalFsCopyBackend {
    file_copier: LocalFileCopier,
    durable_writes: bool,
}

impl LocalFsCopyBackend {
    /// Creates a new LocalFsCopyBackend with default file copier settings.
    pub fn new() -> Self {
        Self {
            file_copier: LocalFileCopier::Os(OsFileCopier),
            durable_writes: false,
        }
    }

    pub fn with_strategy(strategy: CopyStrategy, mode: &Mode) -> Self {
        let durable_writes = matches!(mode, Mode::Migrate);
        let file_copier = match resolve_copy_strategy(strategy, mode) {
            ResolvedCopyStrategy::Os => LocalFileCopier::Os(OsFileCopier),
            ResolvedCopyStrategy::NativePreferred => {
                LocalFileCopier::NativePreferred(NativePreferredFileCopier::new())
            }
        };
        Self {
            file_copier,
            durable_writes,
        }
    }

    pub fn with_transfer_config(config: &TransferConfig) -> Self {
        Self::with_strategy(config.copy_strategy, &config.mode)
    }

    pub fn durable_writes_enabled(&self) -> bool {
        self.durable_writes
    }

    pub fn copy_batch(
        &self,
        batch: &Batch,
        source_root: &Path,
        destination_root: &Path,
        progress: &mut dyn ProgressReporter,
        check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
    ) -> Result<(), CaravanError> {
        copy_batch_with_components_and_durability(
            batch,
            source_root,
            destination_root,
            &self.file_copier,
            &FsDirectoryCreator,
            progress,
            self.durable_writes,
            check_interrupt,
        )
    }
}

impl Default for LocalFsCopyBackend {
    fn default() -> Self {
        Self::new()
    }
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

fn copy_file_atomically(
    file_copier: &dyn FileCopier,
    source_path: &Path,
    destination_path: &Path,
    size_hint: u64,
    durable_writes: bool,
) -> Result<(), CaravanError> {
    let temp_destination_path = temp_destination_path(destination_path);
    clear_stale_temp_file(&temp_destination_path, destination_path)?;

    if let Err(source) =
        file_copier.copy_file_with_size_hint(source_path, &temp_destination_path, Some(size_hint))
    {
        let _ = std::fs::remove_file(&temp_destination_path);
        return Err(CaravanError::IoContext {
            context: format!(
                "failed to copy {} to {}",
                source_path.display(),
                destination_path.display()
            ),
            source,
        });
    }

    if durable_writes {
        if let Err(source) = sync_file_data(&temp_destination_path) {
            let _ = std::fs::remove_file(&temp_destination_path);
            return Err(CaravanError::IoContext {
                context: format!(
                    "failed to flush copied file before finalize {}",
                    destination_path.display()
                ),
                source,
            });
        }
    }

    if let Err(source) = std::fs::rename(&temp_destination_path, destination_path) {
        let _ = std::fs::remove_file(&temp_destination_path);
        return Err(CaravanError::IoContext {
            context: format!(
                "failed to finalize copied file {}",
                destination_path.display()
            ),
            source,
        });
    }

    Ok(())
}

fn temp_destination_path(destination_path: &Path) -> PathBuf {
    let file_name = destination_path
        .file_name()
        .map(|name| {
            let mut temp_name = OsString::from(name);
            temp_name.push(".caravan.part");
            temp_name
        })
        .unwrap_or_else(|| OsString::from(".caravan.part"));

    if let Some(parent) = destination_path.parent() {
        parent.join(file_name)
    } else {
        PathBuf::from(file_name)
    }
}

fn clear_stale_temp_file(temp_path: &Path, destination_path: &Path) -> Result<(), CaravanError> {
    match std::fs::remove_file(temp_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(CaravanError::IoContext {
            context: format!(
                "failed to remove stale temporary file for {} ({})",
                destination_path.display(),
                temp_path.display()
            ),
            source,
        }),
    }
}

fn sync_file_data(path: &Path) -> io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

fn sync_parent_directories(parents: HashSet<PathBuf>) -> Result<(), CaravanError> {
    let mut parents: Vec<PathBuf> = parents.into_iter().collect();
    parents.sort();

    for parent in parents {
        sync_directory(&parent).map_err(|source| CaravanError::IoContext {
            context: format!("failed to flush destination directory {}", parent.display()),
            source,
        })?;
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn sync_directory(path: &Path) -> io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

#[cfg(target_os = "windows")]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}
