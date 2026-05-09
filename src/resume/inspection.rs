use std::path::Path;

use serde::Serialize;

use crate::error::CaravanError;
use crate::models::state::{BatchPhase, MigrationState};

use super::reconciliation::reconcile_batch_destination;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailedBatchInspection {
    pub batch_id: String,
    pub all_destination_files_ready: bool,
    pub missing_in_destination: Vec<String>,
    pub size_mismatches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailedBatchInspectionReport {
    pub failed_batch_count: usize,
    pub failed_batches: Vec<FailedBatchInspection>,
}

/// Build an inspection report for failed batches without mutating state.
pub fn inspect_failed_batches(
    state: &MigrationState,
    destination_root: &Path,
) -> Result<FailedBatchInspectionReport, CaravanError> {
    let failed_batch_ids: Vec<String> = state
        .batches
        .iter()
        .filter(|batch| batch.phase == BatchPhase::Failed && !batch.deleted)
        .map(|batch| batch.batch_id.clone())
        .collect();

    let mut failed_batches = Vec::with_capacity(failed_batch_ids.len());
    for batch_id in failed_batch_ids {
        let batch = state.materialize_planned_batch(&batch_id).ok_or_else(|| {
            CaravanError::StateCorrupt(format!(
                "missing immutable batch manifest for {}; cannot inspect failed batch safely",
                batch_id
            ))
        })?;
        let recon = reconcile_batch_destination(&batch, destination_root);
        failed_batches.push(FailedBatchInspection {
            batch_id,
            all_destination_files_ready: recon.all_destination_files_ready,
            missing_in_destination: recon.missing_in_destination,
            size_mismatches: recon.size_mismatches,
        });
    }

    Ok(FailedBatchInspectionReport {
        failed_batch_count: failed_batches.len(),
        failed_batches,
    })
}
