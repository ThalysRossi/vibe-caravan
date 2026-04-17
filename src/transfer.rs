use std::collections::HashSet;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use crate::config::{CopyStrategy, Mode, TransferConfig};
use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::progress::ProgressReporter;

/// Thread-local buffer pool for reusing allocation buffers between file copies.
/// This eliminates the overhead of allocating large buffers (e.g., 64MiB) for each file.
struct BufferPool {
    /// Buffers of various sizes, keyed by their capacity
    buffers: Mutex<Vec<Vec<u8>>>,
}

impl BufferPool {
    /// Create a new empty buffer pool
    fn new() -> Self {
        Self {
            buffers: Mutex::new(Vec::new()),
        }
    }

    /// Get a buffer of at least the requested size.
    /// Returns a buffer from the pool if available, otherwise allocates a new one.
    fn get_buffer(&self, min_size: usize) -> Vec<u8> {
        let mut buffers = self.lock_buffers();

        // Try to find a buffer with sufficient capacity
        if let Some(index) = buffers.iter().position(|buf| buf.capacity() >= min_size) {
            let mut buffer = buffers.remove(index);
            buffer.clear(); // Clear any existing data
            buffer.resize(min_size, 0); // Ensure it has the right size
            buffer
        } else {
            // Allocate new buffer with exact requested size
            vec![0u8; min_size]
        }
    }

    /// Return a buffer to the pool for reuse.
    /// The pool keeps at most 4 buffers to avoid excessive memory usage.
    fn return_buffer(&self, buffer: Vec<u8>) {
        let mut buffers = self.lock_buffers();

        // Keep at most 4 buffers in the pool (optimized for Ryzen 5 5600X + 16GB RAM)
        if buffers.len() < 4 {
            buffers.push(buffer);
        }
        // If pool is full, buffer is dropped (freed)
    }

    fn lock_buffers(&self) -> MutexGuard<'_, Vec<Vec<u8>>> {
        match self.buffers.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

// Thread-local buffer pool instance
thread_local! {
    static BUFFER_POOL: BufferPool = BufferPool::new();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedCopyStrategy {
    Hybrid,
    NativePreferred,
    Buffered,
}

pub fn resolve_copy_strategy(strategy: CopyStrategy, mode: &Mode) -> ResolvedCopyStrategy {
    match strategy {
        CopyStrategy::Buffered => ResolvedCopyStrategy::Buffered,
        CopyStrategy::Native => ResolvedCopyStrategy::NativePreferred,
        CopyStrategy::Auto => {
            if cfg!(windows) && matches!(mode, Mode::Staging) {
                ResolvedCopyStrategy::NativePreferred
            } else {
                ResolvedCopyStrategy::Hybrid
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

#[cfg(windows)]
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

#[cfg(not(windows))]
fn system_native_copy_file(_source: &Path, _destination: &Path) -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native copy strategy is unavailable on this platform",
    ))
}

/// Native-first copier: tries OS-native copy API and falls back to hybrid copy on failure.
#[derive(Debug, Clone)]
pub struct NativePreferredFileCopier {
    fallback: HybridFileCopier,
}

impl NativePreferredFileCopier {
    pub fn new(buffer_size: usize, threshold: u64) -> Self {
        Self {
            fallback: HybridFileCopier::new(buffer_size, threshold),
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

    /// Default buffer size (16 MiB) - optimized for HDD performance
    pub const DEFAULT_BUFFER_SIZE: usize = 16 * 1024 * 1024;
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

        // Get buffer from pool instead of allocating new one
        let mut buffer = BUFFER_POOL.with(|pool| pool.get_buffer(self.buffer_size));
        let mut total_copied = 0u64;

        loop {
            let bytes_read = source_file.read(&mut buffer)?;
            if bytes_read == 0 {
                break; // EOF
            }

            dest_file.write_all(&buffer[..bytes_read])?;
            total_copied += bytes_read as u64;
        }

        // Return buffer to pool for reuse
        BUFFER_POOL.with(|pool| pool.return_buffer(buffer));

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
        Self {
            buffer_size,
            threshold,
        }
    }

    /// Creates a new hybrid file copier with default values.
    /// - Buffer size: 16 MiB (16 * 1024 * 1024) - optimized for HDD performance
    /// - Threshold: 8 MiB (8 * 1024 * 1024) - files smaller use OS copy
    pub fn with_defaults() -> Self {
        Self::new(
            BufferedFileCopier::DEFAULT_BUFFER_SIZE,
            8 * 1024 * 1024, // 8 MiB
        )
    }

    /// Get the buffer size in bytes
    pub fn buffer_size(&self) -> usize {
        self.buffer_size
    }

    /// Get the threshold in bytes
    pub fn threshold(&self) -> u64 {
        self.threshold
    }
}

impl Default for HybridFileCopier {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl FileCopier for HybridFileCopier {
    fn copy_file(&self, source: &Path, destination: &Path) -> std::io::Result<u64> {
        self.copy_file_with_size_hint(source, destination, None)
    }

    fn copy_file_with_size_hint(
        &self,
        source: &Path,
        destination: &Path,
        size_hint: Option<u64>,
    ) -> std::io::Result<u64> {
        // Prefer caller-provided planned size to avoid extra metadata syscalls.
        let file_size = match size_hint {
            Some(size) => size,
            None => std::fs::metadata(source)?.len(),
        };
        if file_size < self.threshold {
            OsFileCopier.copy_file(source, destination)
        } else {
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
enum LocalFileCopier {
    Hybrid(HybridFileCopier),
    NativePreferred(NativePreferredFileCopier),
    Buffered(BufferedFileCopier),
}

#[derive(Debug, Clone)]
pub struct LocalFsCopyBackend {
    file_copier: LocalFileCopier,
    durable_writes: bool,
}

impl LocalFsCopyBackend {
    /// Creates a new LocalFsCopyBackend with default file copier settings.
    pub fn new() -> Self {
        Self::with_config(
            BufferedFileCopier::DEFAULT_BUFFER_SIZE,
            HybridFileCopier::with_defaults().threshold(),
        )
    }

    /// Creates a new LocalFsCopyBackend with custom buffer size and threshold.
    pub fn with_config(buffer_size: usize, threshold: u64) -> Self {
        Self {
            file_copier: LocalFileCopier::Hybrid(HybridFileCopier::new(buffer_size, threshold)),
            durable_writes: false,
        }
    }

    pub fn with_strategy(
        buffer_size: usize,
        threshold: u64,
        strategy: CopyStrategy,
        mode: &Mode,
    ) -> Self {
        let durable_writes = matches!(mode, Mode::Migrate);
        let file_copier = match resolve_copy_strategy(strategy, mode) {
            ResolvedCopyStrategy::Hybrid => {
                LocalFileCopier::Hybrid(HybridFileCopier::new(buffer_size, threshold))
            }
            ResolvedCopyStrategy::NativePreferred => LocalFileCopier::NativePreferred(
                NativePreferredFileCopier::new(buffer_size, threshold),
            ),
            ResolvedCopyStrategy::Buffered => {
                LocalFileCopier::Buffered(BufferedFileCopier::new(buffer_size))
            }
        };
        Self {
            file_copier,
            durable_writes,
        }
    }

    pub fn with_transfer_config(config: &TransferConfig) -> Self {
        Self::with_strategy(
            config.copy_buffer_size,
            config.buffered_copy_threshold,
            config.copy_strategy,
            &config.mode,
        )
    }

    pub fn durable_writes_enabled(&self) -> bool {
        self.durable_writes
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
            LocalFileCopier::Hybrid(copier) => copier.copy_file(source, destination),
            LocalFileCopier::NativePreferred(copier) => copier.copy_file(source, destination),
            LocalFileCopier::Buffered(copier) => copier.copy_file(source, destination),
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
        self.copy_batch_with_progress(
            batch,
            source_root,
            destination_root,
            &mut crate::progress::NoopProgress,
        )
    }

    fn copy_batch_with_progress(
        &self,
        batch: &Batch,
        source_root: &Path,
        destination_root: &Path,
        progress: &mut dyn ProgressReporter,
    ) -> Result<(), CaravanError> {
        copy_batch_with_components_and_durability(
            batch,
            source_root,
            destination_root,
            &self.file_copier,
            &FsDirectoryCreator,
            progress,
            self.durable_writes,
        )
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
        size_hint: Option<u64>,
    ) -> io::Result<u64> {
        match self {
            LocalFileCopier::Hybrid(copier) => {
                copier.copy_file_with_size_hint(source, destination, size_hint)
            }
            LocalFileCopier::NativePreferred(copier) => copier.copy_file(source, destination),
            LocalFileCopier::Buffered(copier) => copier.copy_file(source, destination),
        }
    }
}

pub fn transfer_batch(
    batch: &Batch,
    source_root: &Path,
    destination_root: &Path,
    backend: &dyn CopyBackend,
) -> Result<(), CaravanError> {
    transfer_batch_with_progress(
        batch,
        source_root,
        destination_root,
        backend,
        &mut crate::progress::NoopProgress,
    )
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

pub fn copy_batch_with_components(
    batch: &Batch,
    source_root: &Path,
    destination_root: &Path,
    file_copier: &dyn FileCopier,
    dir_creator: &dyn DirectoryCreator,
    progress: &mut dyn ProgressReporter,
) -> Result<(), CaravanError> {
    copy_batch_with_components_and_durability(
        batch,
        source_root,
        destination_root,
        file_copier,
        dir_creator,
        progress,
        false,
    )
}

pub fn copy_batch_with_components_and_durability(
    batch: &Batch,
    source_root: &Path,
    destination_root: &Path,
    file_copier: &dyn FileCopier,
    dir_creator: &dyn DirectoryCreator,
    progress: &mut dyn ProgressReporter,
    durable_writes: bool,
) -> Result<(), CaravanError> {
    progress.start(batch.files.len(), "Copying");

    let mut created_dirs = HashSet::new();
    let mut touched_parent_dirs = HashSet::new();

    for (index, file) in batch.files.iter().enumerate() {
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

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}
