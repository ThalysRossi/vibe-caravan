use std::path::Path;

use crate::capacity;
use crate::error::CaravanError;
use crate::models::batch::Batch;

pub(crate) fn ensure_destination_capacity(
    dest: &Path,
    required_bytes: u64,
) -> Result<(), CaravanError> {
    let capacity_report = capacity::check_capacity(dest, required_bytes, 0)?;
    eprintln!(
        "[CAPACITY] {}",
        capacity::format_capacity_decision_trace(dest, &capacity_report)
    );
    if capacity_report.decision == capacity::CapacityDecision::Abort {
        let diagnostics = capacity::inspect_destination_space(dest);
        eprintln!(
            "[CAPACITY] {}",
            capacity::format_destination_space_diagnostic(dest, &capacity_report, &diagnostics)
        );
        eprintln!(
            "Capacity check failed: {}",
            capacity_report.reason.unwrap_or_default()
        );
        return Err(CaravanError::PolicyBlocked(
            "insufficient destination space".to_string(),
        ));
    }

    Ok(())
}

pub(crate) fn ensure_destination_capacity_for_batch(
    dest: &Path,
    batch: &Batch,
) -> Result<(), CaravanError> {
    let cleanup = clear_stale_batch_temp_files(dest, batch)?;
    if cleanup.removed_count > 0 {
        eprintln!(
            "[CAPACITY] removed_stale_temp_files count={} raw_bytes={}",
            cleanup.removed_count, cleanup.removed_bytes
        );
    }

    ensure_destination_capacity(dest, batch.total_bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TempCleanupSummary {
    removed_count: u64,
    removed_bytes: u64,
}

fn clear_stale_batch_temp_files(
    dest: &Path,
    batch: &Batch,
) -> Result<TempCleanupSummary, CaravanError> {
    let mut summary = TempCleanupSummary {
        removed_count: 0,
        removed_bytes: 0,
    };

    for file in &batch.files {
        let final_destination = dest.join(&file.relative_path);
        let temp_destination = crate::transfer::temp_destination_path(&final_destination);
        let size = std::fs::metadata(&temp_destination)
            .map(|metadata| metadata.len())
            .unwrap_or(0);

        match std::fs::remove_file(&temp_destination) {
            Ok(()) => {
                summary.removed_count += 1;
                summary.removed_bytes = summary.removed_bytes.saturating_add(size);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(CaravanError::IoContext {
                    context: format!(
                        "failed to remove stale temporary file before capacity check {}",
                        temp_destination.display()
                    ),
                    source,
                });
            }
        }
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::file_entry::FileEntry;
    use std::path::PathBuf;

    #[test]
    fn ensure_destination_capacity_allows_small_required_bytes() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        ensure_destination_capacity(tmp.path(), 1).expect("small requirement should pass");
    }

    #[test]
    fn ensure_destination_capacity_blocks_when_requirement_is_unreachable() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let err = ensure_destination_capacity(tmp.path(), u64::MAX)
            .expect_err("max requirement should fail");
        assert!(err.to_string().contains("insufficient destination space"));
    }

    #[test]
    fn capacity_for_batch_removes_stale_temp_files_before_checking_space() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(tmp.path().join("nested")).expect("nested dir");
        let stale_temp = tmp.path().join("nested").join("movie.mkv.caravan.part");
        std::fs::write(&stale_temp, b"partial").expect("stale temp");

        let batch = Batch {
            id: "batch-000001".to_string(),
            files: vec![FileEntry {
                relative_path: PathBuf::from("nested").join("movie.mkv"),
                size_bytes: 1,
                modified_time: None,
            }],
            total_bytes: 1,
            file_count: 1,
        };

        ensure_destination_capacity_for_batch(tmp.path(), &batch)
            .expect("capacity check should pass after stale temp cleanup");

        assert!(
            !stale_temp.exists(),
            "stale temp file should be removed before capacity check"
        );
    }
}
