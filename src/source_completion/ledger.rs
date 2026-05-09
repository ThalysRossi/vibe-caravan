use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::atomic_write;
use crate::error::CaravanError;
use crate::migration_registry;
use crate::models::state::CompletedFileIdentity;

pub(super) const LEDGER_FORMAT_VERSION: u32 = 1;
const LEDGER_FILENAME: &str = "source_completed_files.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCompletionLedger {
    pub format_version: u32,
    pub entries: Vec<CompletedFileIdentity>,
}

impl SourceCompletionLedger {
    pub fn new() -> Self {
        Self {
            format_version: LEDGER_FORMAT_VERSION,
            entries: Vec::new(),
        }
    }
}

impl Default for SourceCompletionLedger {
    fn default() -> Self {
        Self::new()
    }
}

pub fn ledger_path(source_root: &Path) -> PathBuf {
    migration_registry::state_dir_in_source(source_root).join(LEDGER_FILENAME)
}

pub fn load_ledger(source_root: &Path) -> Result<SourceCompletionLedger, CaravanError> {
    let path = ledger_path(source_root);
    if !path.exists() {
        return Ok(SourceCompletionLedger::new());
    }

    let payload = fs::read_to_string(&path).map_err(|source| CaravanError::StateRead {
        path: path.clone(),
        source,
    })?;
    let ledger: SourceCompletionLedger =
        serde_json::from_str(&payload).map_err(|source| CaravanError::StateParse {
            path: path.clone(),
            source,
        })?;

    if ledger.format_version != LEDGER_FORMAT_VERSION {
        return Err(CaravanError::StateCorrupt(format!(
            "unsupported source completion ledger format version {} in {}",
            ledger.format_version,
            path.display()
        )));
    }

    Ok(ledger)
}

pub fn persist_ledger(
    source_root: &Path,
    ledger: &SourceCompletionLedger,
) -> Result<(), CaravanError> {
    let payload = serde_json::to_vec_pretty(ledger).map_err(|err| {
        CaravanError::StateCorrupt(format!(
            "failed to serialize source completion ledger: {err}"
        ))
    })?;
    atomic_write::write_bytes(
        &ledger_path(source_root),
        &payload,
        "source completion ledger",
    )
}
