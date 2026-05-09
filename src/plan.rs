use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::file_entry::FileEntry;
use crate::models::state::PlannedBatch;
use crate::scan::scan_source;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanOptions {
    pub batch_size_bytes: u64,
    pub max_files: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningSnapshot {
    pub source_file_count: usize,
    pub source_total_bytes: u64,
    pub batches: Vec<Batch>,
}

pub fn build_plan(
    source_root: &Path,
    options: &PlanOptions,
) -> Result<PlanningSnapshot, CaravanError> {
    if options.batch_size_bytes == 0 {
        return Err(CaravanError::InvalidArguments(
            "batch-size must be greater than zero".to_string(),
        ));
    }
    if options.max_files == Some(0) {
        return Err(CaravanError::InvalidArguments(
            "max-files must be greater than zero when provided".to_string(),
        ));
    }

    let entries = scan_source(source_root)?;
    build_plan_from_entries(entries, options)
}

pub fn build_plan_from_entries(
    entries: Vec<FileEntry>,
    options: &PlanOptions,
) -> Result<PlanningSnapshot, CaravanError> {
    let source_total_bytes = entries.iter().map(|file| file.size_bytes).sum::<u64>();
    let batches = plan_batches(entries, options)?;

    Ok(PlanningSnapshot {
        source_file_count: batches.iter().map(|batch| batch.file_count).sum(),
        source_total_bytes,
        batches,
    })
}

pub fn plan_batches(
    entries: Vec<FileEntry>,
    options: &PlanOptions,
) -> Result<Vec<Batch>, CaravanError> {
    if options.batch_size_bytes == 0 {
        return Err(CaravanError::InvalidArguments(
            "batch-size must be greater than zero".to_string(),
        ));
    }
    if options.max_files == Some(0) {
        return Err(CaravanError::InvalidArguments(
            "max-files must be greater than zero when provided".to_string(),
        ));
    }

    let mut sorted = entries;
    sorted.sort_by(cmp_for_planning);

    build_batches_from_ordered_entries(sorted, options)
}

fn build_batches_from_ordered_entries(
    sorted: Vec<FileEntry>,
    options: &PlanOptions,
) -> Result<Vec<Batch>, CaravanError> {
    let mut batches = Vec::new();
    let mut current_files: Vec<FileEntry> = Vec::new();
    let mut current_bytes: u64 = 0;
    let max_files = options.max_files.unwrap_or(usize::MAX);

    for entry in sorted {
        if entry.size_bytes > options.batch_size_bytes {
            if !current_files.is_empty() {
                batches.push(make_batch(
                    batches.len() + 1,
                    std::mem::take(&mut current_files),
                    current_bytes,
                ));
                current_bytes = 0;
            }
            let single_size = entry.size_bytes;
            batches.push(make_batch(batches.len() + 1, vec![entry], single_size));
            continue;
        }

        let exceeds_size =
            current_bytes.saturating_add(entry.size_bytes) > options.batch_size_bytes;
        let exceeds_files = current_files.len() >= max_files;
        if !current_files.is_empty() && (exceeds_size || exceeds_files) {
            batches.push(make_batch(
                batches.len() + 1,
                std::mem::take(&mut current_files),
                current_bytes,
            ));
            current_bytes = 0;
        }

        current_bytes = current_bytes.saturating_add(entry.size_bytes);
        current_files.push(entry);
    }

    if !current_files.is_empty() {
        batches.push(make_batch(batches.len() + 1, current_files, current_bytes));
    }

    Ok(batches)
}

fn cmp_for_planning(a: &FileEntry, b: &FileEntry) -> std::cmp::Ordering {
    let a_parent = a.relative_path.parent().unwrap_or(Path::new(""));
    let b_parent = b.relative_path.parent().unwrap_or(Path::new(""));
    a_parent
        .cmp(b_parent)
        .then_with(|| a.relative_path.cmp(&b.relative_path))
}

/// Load an individual batch definition from disk for resume
///
/// When resuming we avoid rebuilding the whole plan which would generate different batch IDs,
/// instead we scan the source again and find the exact batch matching the ID we need.
pub fn load_batch_definition(
    source_root: &Path,
    batch_id: &str,
    batch_size_bytes: u64,
    max_files: Option<u64>,
) -> Result<Batch, CaravanError> {
    // We scan source and rebuild batches to find the one with matching ID
    // This works because batch IDs are deterministic and reproducible
    let entries = scan_source(source_root)?;

    // ✅ Use the EXACT original batch size that was used when planning!
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

fn make_batch(index: usize, files: Vec<FileEntry>, total_bytes: u64) -> Batch {
    Batch {
        id: format!("batch-{index:06}"),
        file_count: files.len(),
        files,
        total_bytes,
    }
}

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
