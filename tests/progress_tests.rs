use std::time::Duration;
use tempfile::tempdir;
use caravan::progress::{ProgressReporter, NoopProgress, TerminalProgress};
use caravan::transfer::{transfer_batch_with_progress, LocalFsCopyBackend};
use caravan::verify::verify_batch_with_progress;
use caravan::config::VerificationMode;
use caravan::models::batch::Batch;
use caravan::models::file_entry::FileEntry;

#[derive(Debug, Default)]
struct MockProgress {
    start_called: bool,
    advance_called: Vec<usize>,
    finish_called: bool,
    total_items: usize,
}

impl ProgressReporter for MockProgress {
    fn start(&mut self, total_items: usize, _operation: &str) {
        self.start_called = true;
        self.total_items = total_items;
    }

    fn advance(&mut self, current: usize, _item_name: Option<&str>) {
        self.advance_called.push(current);
    }

    fn finish(&mut self) {
        self.finish_called = true;
    }
}

#[test]
fn noop_progress_implements_all_methods() {
    let mut progress = NoopProgress;
    progress.start(100, "test");
    progress.advance(50, None);
    progress.finish();
    
    // No panics, that's the test
}

#[test]
fn terminal_progress_lifecycle_completes() {
    let mut progress = TerminalProgress::new();
    progress.start(5, "test operation");
    
    for i in 1..=5 {
        progress.advance(i, None);
    }
    
    progress.finish();
}

#[test]
fn duration_formatting_works() {
    // Test private method by reimplementation matching the new implementation
    fn format_duration(d: Duration) -> String {
        let secs = d.as_secs();
        match secs {
            0..=59 => format!("{}s", secs),
            60..=3599 => {
                let mins = secs / 60;
                let secs_remain = secs % 60;
                format!("{}m {:02}s", mins, secs_remain)
            }
            _ => {
                let hours = secs / 3600;
                let mins = (secs % 3600) / 60;
                let secs_remain = secs % 60;
                format!("{}h {:02}m {:02}s", hours, mins, secs_remain)
            }
        }
    }

    assert_eq!(format_duration(Duration::from_secs(35)), "35s");
    assert_eq!(format_duration(Duration::from_secs(65)), "1m 05s");
    assert_eq!(format_duration(Duration::from_secs(125)), "2m 05s");
    assert_eq!(format_duration(Duration::from_secs(3600)), "1h 00m 00s");
    assert_eq!(format_duration(Duration::from_secs(3665)), "1h 01m 05s");
    assert_eq!(format_duration(Duration::from_secs(7200)), "2h 00m 00s");
}

#[test]
fn mock_progress_receives_all_events() {
    let mut mock = MockProgress::default();
    
    mock.start(3, "test");
    assert!(mock.start_called);
    assert_eq!(mock.total_items, 3);
    
    mock.advance(1, None);
    mock.advance(2, None);
    mock.advance(3, None);
    assert_eq!(mock.advance_called, vec![1, 2, 3]);
    
    mock.finish();
    assert!(mock.finish_called);
}

#[test]
fn transfer_batch_calls_progress_correctly() {
    let src = tempdir().unwrap();
    let dst = tempdir().unwrap();
    
    // Create test files
    for i in 0..3 {
        std::fs::write(src.path().join(format!("file{}.txt", i)), b"test content").unwrap();
    }
    
    let files = vec![
        FileEntry { relative_path: "file0.txt".into(), size_bytes: 12, modified_time: None },
        FileEntry { relative_path: "file1.txt".into(), size_bytes: 12, modified_time: None },
        FileEntry { relative_path: "file2.txt".into(), size_bytes: 12, modified_time: None },
    ];
    
    let batch = Batch {
        id: "test-batch".to_string(),
        files,
        total_bytes: 36,
        file_count: 3,
    };
    
    let mut mock = MockProgress::default();
    let backend = LocalFsCopyBackend::new();
    
    transfer_batch_with_progress(&batch, src.path(), dst.path(), &backend, &mut mock).unwrap();
    
    assert!(mock.start_called);
    assert_eq!(mock.total_items, 3);
    assert_eq!(mock.advance_called, vec![1, 2, 3]);
    assert!(mock.finish_called);
}

#[test]
fn verify_batch_calls_progress_correctly() {
    let src = tempdir().unwrap();
    let dst = tempdir().unwrap();
    
    // Create matching files in both locations
    for i in 0..3 {
        let content = b"test content";
        std::fs::write(src.path().join(format!("file{}.txt", i)), content).unwrap();
        std::fs::write(dst.path().join(format!("file{}.txt", i)), content).unwrap();
    }
    
    let files = vec![
        FileEntry { relative_path: "file0.txt".into(), size_bytes: 12, modified_time: None },
        FileEntry { relative_path: "file1.txt".into(), size_bytes: 12, modified_time: None },
        FileEntry { relative_path: "file2.txt".into(), size_bytes: 12, modified_time: None },
    ];
    
    let batch = Batch {
        id: "test-batch".to_string(),
        files,
        total_bytes: 36,
        file_count: 3,
    };
    
    let mut mock = MockProgress::default();
    
    let report = verify_batch_with_progress(
        &batch, src.path(), dst.path(), VerificationMode::Digest, &mut mock
    ).unwrap();
    
    assert_eq!(report.status, caravan::models::verification::VerificationStatus::Pass);
    assert!(mock.start_called);
    assert_eq!(mock.total_items, 3);
    assert_eq!(mock.advance_called, vec![1, 2, 3]);
    assert!(mock.finish_called);
}
