use std::path::PathBuf;

use caravan::models::file_entry::FileEntry;
use caravan::models::state::CompletedFileIdentity;
use caravan::progress::ProgressReporter;
use caravan::source_completion::{
    SourceCompletionLedger, filter_entries_for_new_migration_selective,
    filter_entries_for_persisted_skips_selective, hash_source_entries_with_progress,
    hash_source_entries_with_progress_and_interrupt, persist_ledger,
};

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

#[test]
fn hashing_shutdown_does_not_finish_progress() {
    let tmp = tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("a.txt"), b"aaaa").expect("write a");
    let entries = vec![entry("a.txt", 4)];
    let mut progress = RecordingProgress::default();
    let mut check_interrupt = || Err(caravan::error::CaravanError::GracefulShutdown);

    let err = hash_source_entries_with_progress_and_interrupt(
        tmp.path(),
        &entries,
        &mut progress,
        &mut check_interrupt,
    )
    .expect_err("shutdown should interrupt hashing");

    assert!(matches!(
        err,
        caravan::error::CaravanError::GracefulShutdown
    ));
    assert_eq!(progress.starts, vec![(1, "Hashing source".to_string())]);
    assert!(progress.advances.is_empty());
    assert_eq!(progress.finish_count, 0);
}

#[test]
fn selective_new_migration_hashes_only_ledger_candidates() {
    let tmp = tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("a.txt"), b"aaaa").expect("write a");
    let mut ledger = SourceCompletionLedger::new();
    ledger.entries.push(CompletedFileIdentity {
        mode: "staging".to_string(),
        relative_path: PathBuf::from("a.txt"),
        size_bytes: 4,
        blake3_hash: file_hash_hex(&tmp.path().join("a.txt")),
    });
    persist_ledger(tmp.path(), &ledger).expect("persist ledger");

    let entries = vec![entry("a.txt", 4), entry("missing-noncandidate.txt", 7)];
    let mut progress = RecordingProgress::default();
    let mut no_interrupt = || Ok(());

    let filtered = filter_entries_for_new_migration_selective(
        tmp.path(),
        "staging",
        entries,
        &mut progress,
        &mut no_interrupt,
    )
    .expect("selective filtering should not hash non-candidates");

    assert_eq!(
        progress.starts,
        vec![(1, "Hashing source candidates".to_string())]
    );
    assert_eq!(progress.total_bytes, vec![4]);
    assert_eq!(progress.advances, vec![(1, Some("a.txt".to_string()))]);
    assert_eq!(filtered.skipped_completed_files.len(), 1);
    assert_eq!(filtered.entries_to_plan.len(), 1);
    assert_eq!(
        filtered.entries_to_plan[0].relative_path,
        PathBuf::from("missing-noncandidate.txt")
    );
}

#[test]
fn selective_new_migration_skips_hash_progress_when_no_candidates() {
    let tmp = tempdir().expect("tempdir");
    let entries = vec![entry("missing-noncandidate.txt", 7)];
    let mut progress = RecordingProgress::default();
    let mut no_interrupt = || Ok(());

    let filtered = filter_entries_for_new_migration_selective(
        tmp.path(),
        "staging",
        entries,
        &mut progress,
        &mut no_interrupt,
    )
    .expect("no candidates should not require hashing");

    assert!(progress.starts.is_empty());
    assert!(progress.total_bytes.is_empty());
    assert!(progress.advances.is_empty());
    assert_eq!(filtered.skipped_completed_files.len(), 0);
    assert_eq!(filtered.entries_to_plan.len(), 1);
}

#[test]
fn selective_persisted_skip_hashes_only_skipped_files() {
    let tmp = tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("a.txt"), b"aaaa").expect("write a");
    let skipped = CompletedFileIdentity {
        mode: "staging".to_string(),
        relative_path: PathBuf::from("a.txt"),
        size_bytes: 4,
        blake3_hash: file_hash_hex(&tmp.path().join("a.txt")),
    };
    let entries = vec![entry("a.txt", 4), entry("missing-noncandidate.txt", 7)];
    let mut progress = RecordingProgress::default();
    let mut no_interrupt = || Ok(());

    let filtered = filter_entries_for_persisted_skips_selective(
        tmp.path(),
        "staging",
        entries,
        &[skipped],
        &mut progress,
        &mut no_interrupt,
    )
    .expect("selective filtering should only hash skipped files");

    assert_eq!(
        progress.starts,
        vec![(1, "Hashing source candidates".to_string())]
    );
    assert_eq!(progress.total_bytes, vec![4]);
    assert_eq!(progress.advances, vec![(1, Some("a.txt".to_string()))]);
    assert_eq!(filtered.len(), 1);
    assert_eq!(
        filtered[0].relative_path,
        PathBuf::from("missing-noncandidate.txt")
    );
}

fn file_hash_hex(path: &std::path::Path) -> String {
    let hash = caravan::verify::digest_file(path).expect("hash file");
    let mut rendered = String::with_capacity(64);
    for byte in hash {
        rendered.push_str(&format!("{byte:02x}"));
    }
    rendered
}
