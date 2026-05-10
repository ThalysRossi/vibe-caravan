use std::path::PathBuf;

use caravan::models::file_entry::FileEntry;
use caravan::progress::ProgressReporter;
use caravan::source_completion::hash_source_entries_with_progress;

use tempfile::tempdir;

#[derive(Debug, Default)]
struct RecordingProgress {
    total_bytes: Vec<u64>,
    starts: Vec<(usize, String)>,
    advances: Vec<(usize, Option<String>)>,
    finish_count: usize,
}

impl ProgressReporter for RecordingProgress {
    fn set_total_bytes(&mut self, total_bytes: u64) {
        self.total_bytes.push(total_bytes);
    }

    fn start(&mut self, total_items: usize, operation: &str) {
        self.starts.push((total_items, operation.to_string()));
    }

    fn advance(&mut self, current: usize, item_name: Option<&str>) {
        self.advances
            .push((current, item_name.map(|name| name.to_string())));
    }

    fn finish(&mut self) {
        self.finish_count += 1;
    }
}

fn entry(relative_path: &str, size_bytes: u64) -> FileEntry {
    FileEntry {
        relative_path: PathBuf::from(relative_path),
        size_bytes,
        modified_time: None,
    }
}

#[test]
fn hashing_reports_total_bytes_start_each_file_and_finish() {
    let tmp = tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("a.txt"), b"aaaa").expect("write a");
    std::fs::write(tmp.path().join("b.txt"), b"bbbbbb").expect("write b");
    let entries = vec![entry("a.txt", 4), entry("b.txt", 6)];
    let mut progress = RecordingProgress::default();

    let hashed = hash_source_entries_with_progress(tmp.path(), &entries, &mut progress)
        .expect("hashing should succeed");

    assert_eq!(hashed.len(), 2);
    assert_eq!(progress.total_bytes, vec![10]);
    assert_eq!(progress.starts, vec![(2, "Hashing source".to_string())]);
    assert_eq!(
        progress.advances,
        vec![
            (1, Some("a.txt".to_string())),
            (2, Some("b.txt".to_string()))
        ]
    );
    assert_eq!(progress.finish_count, 1);
}

#[test]
fn hashing_empty_input_starts_and_finishes_without_advancing() {
    let tmp = tempdir().expect("tempdir");
    let entries = Vec::new();
    let mut progress = RecordingProgress::default();

    let hashed = hash_source_entries_with_progress(tmp.path(), &entries, &mut progress)
        .expect("empty hashing should succeed");

    assert!(hashed.is_empty());
    assert_eq!(progress.total_bytes, vec![0]);
    assert_eq!(progress.starts, vec![(0, "Hashing source".to_string())]);
    assert!(progress.advances.is_empty());
    assert_eq!(progress.finish_count, 1);
}

#[test]
fn hashing_error_does_not_finish_after_partial_progress() {
    let tmp = tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("a.txt"), b"aaaa").expect("write a");
    let entries = vec![entry("a.txt", 4), entry("missing.txt", 7)];
    let mut progress = RecordingProgress::default();

    let err = hash_source_entries_with_progress(tmp.path(), &entries, &mut progress)
        .expect_err("missing file should fail hashing");

    assert!(
        err.to_string().contains("missing.txt"),
        "error should identify missing source file: {err}"
    );
    assert_eq!(progress.total_bytes, vec![11]);
    assert_eq!(progress.starts, vec![(2, "Hashing source".to_string())]);
    assert_eq!(progress.advances, vec![(1, Some("a.txt".to_string()))]);
    assert_eq!(progress.finish_count, 0);
}
