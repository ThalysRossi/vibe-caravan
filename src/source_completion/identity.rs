use std::path::{Path, PathBuf};

use crate::error::CaravanError;
use crate::models::file_entry::FileEntry;
use crate::models::state::CompletedFileIdentity;
use crate::progress::{NoopProgress, ProgressReporter};
use crate::verify;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashedFileEntry {
    pub relative_path: PathBuf,
    pub size_bytes: u64,
    pub blake3_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct FileIdentityKey {
    mode: String,
    relative_path: PathBuf,
    size_bytes: u64,
    blake3_hash: String,
}

pub fn hash_source_entries(
    source_root: &Path,
    entries: &[FileEntry],
) -> Result<Vec<HashedFileEntry>, CaravanError> {
    let mut progress = NoopProgress;
    let mut no_interrupt = || Ok(());
    hash_source_entries_with_options(
        source_root,
        entries,
        &mut progress,
        &mut no_interrupt,
        "Hashing source",
        false,
    )
}

pub fn hash_source_entries_with_progress(
    source_root: &Path,
    entries: &[FileEntry],
    progress: &mut dyn ProgressReporter,
) -> Result<Vec<HashedFileEntry>, CaravanError> {
    let mut no_interrupt = || Ok(());
    hash_source_entries_with_options(
        source_root,
        entries,
        progress,
        &mut no_interrupt,
        "Hashing source",
        false,
    )
}

pub fn hash_source_entries_with_progress_and_interrupt(
    source_root: &Path,
    entries: &[FileEntry],
    progress: &mut dyn ProgressReporter,
    check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
) -> Result<Vec<HashedFileEntry>, CaravanError> {
    hash_source_entries_with_options(
        source_root,
        entries,
        progress,
        check_interrupt,
        "Hashing source",
        false,
    )
}

pub(super) fn hash_source_candidates_with_progress_and_interrupt(
    source_root: &Path,
    entries: &[FileEntry],
    progress: &mut dyn ProgressReporter,
    check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
) -> Result<Vec<HashedFileEntry>, CaravanError> {
    hash_source_entries_with_options(
        source_root,
        entries,
        progress,
        check_interrupt,
        "Hashing source candidates",
        true,
    )
}

fn hash_source_entries_with_options(
    source_root: &Path,
    entries: &[FileEntry],
    progress: &mut dyn ProgressReporter,
    check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
    operation: &str,
    skip_progress_when_empty: bool,
) -> Result<Vec<HashedFileEntry>, CaravanError> {
    if entries.is_empty() && skip_progress_when_empty {
        return Ok(Vec::new());
    }

    let total_bytes = entries
        .iter()
        .fold(0_u64, |total, entry| total.saturating_add(entry.size_bytes));
    progress.set_total_bytes(total_bytes);
    progress.start(entries.len(), operation);

    let mut hashed_entries = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        check_interrupt()?;
        let source_path = source_root.join(&entry.relative_path);
        let blake3_hash = hash_file_hex_with_interrupt(&source_path, check_interrupt)?;
        hashed_entries.push(HashedFileEntry {
            relative_path: entry.relative_path.clone(),
            size_bytes: entry.size_bytes,
            blake3_hash,
        });
        progress.advance(index + 1, Some(&entry.relative_path.to_string_lossy()));
        check_interrupt()?;
    }

    progress.finish();
    Ok(hashed_entries)
}

pub(super) fn identity_from_hashed_entry(
    mode: &str,
    entry: &HashedFileEntry,
) -> CompletedFileIdentity {
    CompletedFileIdentity {
        mode: mode.to_string(),
        relative_path: entry.relative_path.clone(),
        size_bytes: entry.size_bytes,
        blake3_hash: entry.blake3_hash.clone(),
    }
}

pub(super) fn file_key_from_completed_identity(
    identity: &CompletedFileIdentity,
) -> FileIdentityKey {
    FileIdentityKey {
        mode: identity.mode.clone(),
        relative_path: identity.relative_path.clone(),
        size_bytes: identity.size_bytes,
        blake3_hash: identity.blake3_hash.clone(),
    }
}

pub(super) fn file_key_from_hashed_entry(mode: &str, entry: &HashedFileEntry) -> FileIdentityKey {
    FileIdentityKey {
        mode: mode.to_string(),
        relative_path: entry.relative_path.clone(),
        size_bytes: entry.size_bytes,
        blake3_hash: entry.blake3_hash.clone(),
    }
}

pub(super) fn hash_file_hex_with_interrupt(
    path: &Path,
    check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
) -> Result<String, CaravanError> {
    Ok(hex_lower(&verify::digest_file_with_interrupt(
        path,
        check_interrupt,
    )?))
}

fn hex_lower(bytes: &[u8; 32]) -> String {
    let mut rendered = String::with_capacity(64);
    for byte in bytes {
        rendered.push_str(&format!("{byte:02x}"));
    }
    rendered
}
