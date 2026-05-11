use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::state::CompletedFileIdentity;

use super::identity::hash_file_hex_with_interrupt;
use super::ledger::{load_ledger, persist_ledger};

pub fn mark_batch_completed(
    source_root: &Path,
    mode: &str,
    batch: &Batch,
) -> Result<(), CaravanError> {
    let mut no_interrupt = || Ok(());
    mark_batch_completed_with_interrupt(source_root, mode, batch, &mut no_interrupt)
}

pub fn mark_batch_completed_with_interrupt(
    source_root: &Path,
    mode: &str,
    batch: &Batch,
    check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
) -> Result<(), CaravanError> {
    let identities =
        identities_from_batch_with_interrupt(source_root, mode, batch, check_interrupt)?;
    upsert_identities(source_root, &identities)
}

pub fn remove_batch_completed(
    source_root: &Path,
    mode: &str,
    batch: &Batch,
) -> Result<(), CaravanError> {
    if batch.files.is_empty() {
        return Ok(());
    }

    let paths_to_remove: HashSet<PathBuf> = batch
        .files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect();
    let mut ledger = load_ledger(source_root)?;
    let original_len = ledger.entries.len();
    ledger.entries.retain(|identity| {
        identity.mode != mode || !paths_to_remove.contains(&identity.relative_path)
    });
    if ledger.entries.len() != original_len {
        persist_ledger(source_root, &ledger)?;
    }
    Ok(())
}

fn identities_from_batch_with_interrupt(
    source_root: &Path,
    mode: &str,
    batch: &Batch,
    check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
) -> Result<Vec<CompletedFileIdentity>, CaravanError> {
    let mut identities = Vec::with_capacity(batch.files.len());
    for file in &batch.files {
        check_interrupt()?;
        let source_path = source_root.join(&file.relative_path);
        identities.push(CompletedFileIdentity {
            mode: mode.to_string(),
            relative_path: file.relative_path.clone(),
            size_bytes: file.size_bytes,
            blake3_hash: hash_file_hex_with_interrupt(&source_path, check_interrupt)?,
        });
    }
    Ok(identities)
}

fn upsert_identities(
    source_root: &Path,
    identities: &[CompletedFileIdentity],
) -> Result<(), CaravanError> {
    if identities.is_empty() {
        return Ok(());
    }

    let mut ledger = load_ledger(source_root)?;
    let mut completed_keys: HashSet<CompletedFileIdentity> =
        ledger.entries.iter().cloned().collect();
    for identity in identities {
        if completed_keys.insert(identity.clone()) {
            ledger.entries.push(identity.clone());
        }
    }
    persist_ledger(source_root, &ledger)
}
