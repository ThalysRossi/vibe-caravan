use std::fs;
use std::path::Path;

use crate::models::batch::Batch;

/// Result of comparing the batch manifest to the destination tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationResult {
    pub all_destination_files_ready: bool,
    pub missing_in_destination: Vec<String>,
    pub size_mismatches: Vec<String>,
}

pub fn reconciliation_summary(recon: &ReconciliationResult) -> String {
    format!(
        "missing_in_destination={:?}; size_mismatches={:?}",
        recon.missing_in_destination, recon.size_mismatches
    )
}

/// For each file in the batch, check destination exists and size matches the planned entry.
pub fn reconcile_batch_destination(batch: &Batch, dest_root: &Path) -> ReconciliationResult {
    let mut missing_in_destination = Vec::new();
    let mut size_mismatches = Vec::new();

    for entry in &batch.files {
        let rel = entry.relative_path.to_string_lossy().to_string();
        let dest_path = dest_root.join(&entry.relative_path);
        match fs::metadata(&dest_path) {
            Ok(meta) => {
                if !meta.is_file() {
                    missing_in_destination.push(rel);
                } else if meta.len() != entry.size_bytes {
                    size_mismatches.push(rel);
                }
            }
            Err(_) => missing_in_destination.push(rel),
        }
    }

    let all_destination_files_ready =
        missing_in_destination.is_empty() && size_mismatches.is_empty();

    ReconciliationResult {
        all_destination_files_ready,
        missing_in_destination,
        size_mismatches,
    }
}
