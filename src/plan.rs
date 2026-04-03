use std::path::Path;

use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::file_entry::FileEntry;
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

pub fn build_plan(source_root: &Path, options: &PlanOptions) -> Result<PlanningSnapshot, CaravanError> {
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
    let source_total_bytes = entries.iter().map(|f| f.size_bytes).sum::<u64>();
    let batches = plan_batches(entries, options)?;

    Ok(PlanningSnapshot {
        source_file_count: batches.iter().map(|b| b.file_count).sum(),
        source_total_bytes,
        batches,
    })
}

pub fn plan_batches(entries: Vec<FileEntry>, options: &PlanOptions) -> Result<Vec<Batch>, CaravanError> {
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
    sorted.sort_by(|a, b| {
        let a_parent = a.relative_path.parent().unwrap_or(Path::new(""));
        let b_parent = b.relative_path.parent().unwrap_or(Path::new(""));
        a_parent
            .cmp(b_parent)
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });

    let mut batches = Vec::new();
    let mut current_files: Vec<FileEntry> = Vec::new();
    let mut current_bytes: u64 = 0;
    let max_files = options.max_files.unwrap_or(usize::MAX);

    for entry in sorted {
        if entry.size_bytes > options.batch_size_bytes {
            if !current_files.is_empty() {
                batches.push(make_batch(batches.len() + 1, std::mem::take(&mut current_files), current_bytes));
                current_bytes = 0;
            }
            let single_size = entry.size_bytes;
            batches.push(make_batch(batches.len() + 1, vec![entry], single_size));
            continue;
        }

        let exceeds_size = current_bytes.saturating_add(entry.size_bytes) > options.batch_size_bytes;
        let exceeds_files = current_files.len() >= max_files;
        if !current_files.is_empty() && (exceeds_size || exceeds_files) {
            batches.push(make_batch(batches.len() + 1, std::mem::take(&mut current_files), current_bytes));
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

fn make_batch(index: usize, files: Vec<FileEntry>, total_bytes: u64) -> Batch {
    Batch {
        id: format!("batch-{index:06}"),
        file_count: files.len(),
        files,
        total_bytes,
    }
}
