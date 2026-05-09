use std::path::Path;

use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::scan::scan_source;

use super::batching::plan_batches;
use super::types::PlanOptions;

/// Load an individual batch definition from disk for resume.
///
/// When resuming we avoid rebuilding the whole plan which would generate different batch IDs,
/// instead we scan the source again and find the exact batch matching the ID we need.
pub fn load_batch_definition(
    source_root: &Path,
    batch_id: &str,
    batch_size_bytes: u64,
    max_files: Option<u64>,
) -> Result<Batch, CaravanError> {
    let entries = scan_source(source_root)?;
    let opts = PlanOptions {
        batch_size_bytes,
        max_files: max_files.map(|value| value as usize),
    };

    let batches = plan_batches(entries, &opts)?;

    batches
        .into_iter()
        .find(|b| b.id == batch_id)
        .ok_or_else(|| {
            CaravanError::InvalidArguments(format!(
                "Could not locate batch {} in source directory",
                batch_id
            ))
        })
}
