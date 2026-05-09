use std::collections::{HashMap, HashSet};

use crate::error::CaravanError;
use crate::models::state::PlannedBatch;

use super::types::PlanningSnapshot;

pub fn planned_batches_from_snapshot(snapshot: &PlanningSnapshot) -> Vec<PlannedBatch> {
    snapshot
        .batches
        .iter()
        .map(PlannedBatch::from_batch)
        .collect()
}

pub fn ensure_manifest_matches_snapshot(
    persisted_manifest: &[PlannedBatch],
    current_snapshot: &PlanningSnapshot,
) -> Result<(), CaravanError> {
    if persisted_manifest.len() != current_snapshot.batches.len() {
        return Err(CaravanError::InvalidArguments(format!(
            "source drift detected: planned manifest batch count changed (state={}, current={})",
            persisted_manifest.len(),
            current_snapshot.batches.len()
        )));
    }

    let persisted_by_id: HashMap<&str, &PlannedBatch> = persisted_manifest
        .iter()
        .map(|batch| (batch.batch_id.as_str(), batch))
        .collect();

    let mut seen_ids: HashSet<&str> = HashSet::new();
    for batch in &current_snapshot.batches {
        let Some(persisted) = persisted_by_id.get(batch.id.as_str()) else {
            return Err(CaravanError::InvalidArguments(format!(
                "source drift detected: current plan contains unknown batch id {}",
                batch.id
            )));
        };
        seen_ids.insert(batch.id.as_str());

        if persisted.file_count != batch.file_count || persisted.total_bytes != batch.total_bytes {
            return Err(CaravanError::InvalidArguments(format!(
                "source drift detected for {}: batch metadata changed (files {}->{}, bytes {}->{})",
                batch.id,
                persisted.file_count,
                batch.file_count,
                persisted.total_bytes,
                batch.total_bytes
            )));
        }

        if persisted.files.len() != batch.files.len() {
            return Err(CaravanError::InvalidArguments(format!(
                "source drift detected for {}: file list length changed (state={}, current={})",
                batch.id,
                persisted.files.len(),
                batch.files.len()
            )));
        }

        for (index, (persisted_file, current_file)) in
            persisted.files.iter().zip(batch.files.iter()).enumerate()
        {
            if persisted_file.relative_path != current_file.relative_path {
                return Err(CaravanError::InvalidArguments(format!(
                    "source drift detected for {} at file #{index}: path changed (state='{}', current='{}')",
                    batch.id,
                    persisted_file.relative_path.display(),
                    current_file.relative_path.display()
                )));
            }

            if persisted_file.size_bytes != current_file.size_bytes {
                return Err(CaravanError::InvalidArguments(format!(
                    "source drift detected for {} at {}: size changed (state={}, current={})",
                    batch.id,
                    current_file.relative_path.display(),
                    persisted_file.size_bytes,
                    current_file.size_bytes
                )));
            }
        }
    }

    for persisted in persisted_manifest {
        if !seen_ids.contains(persisted.batch_id.as_str()) {
            return Err(CaravanError::InvalidArguments(format!(
                "source drift detected: persisted manifest contains stale batch id {}",
                persisted.batch_id
            )));
        }
    }

    Ok(())
}
