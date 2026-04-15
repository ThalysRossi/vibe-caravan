use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::time::SystemTime;

use tempfile::TempDir;

use caravan::conflict::detect_batch_conflicts;
use caravan::models::batch::Batch;
use caravan::models::file_entry::FileEntry;

// Helper function to create a test batch similar to the one in src/conflict.rs
fn create_test_batch() -> Batch {
    Batch {
        id: "test-batch".to_string(),
        files: vec![
            FileEntry {
                relative_path: PathBuf::from("file1.txt"),
                size_bytes: 10,
                modified_time: Some(SystemTime::now()),
            },
            FileEntry {
                relative_path: PathBuf::from("dir/file2.txt"),
                size_bytes: 20,
                modified_time: Some(SystemTime::now()),
            },
        ],
        total_bytes: 30,
        file_count: 2,
    }
}

#[test]
fn test_empty_destination_no_conflicts() {
    let batch = create_test_batch();
    let dest = TempDir::new().expect("temp dir");

    let report = detect_batch_conflicts(&batch, dest.path()).expect("should succeed");

    assert!(!report.has_conflicts);
    assert_eq!(report.total_conflicts, 0);
    assert!(report.existing_files.is_empty());
    assert!(report.size_mismatches.is_empty());
    assert_eq!(report.scanned_parent_directories, 2);
}

#[test]
fn test_existing_regular_file_conflict() {
    let batch = create_test_batch();
    let dest = TempDir::new().expect("temp dir");

    // Create one of the files in destination
    fs::write(dest.path().join("file1.txt"), b"different content").expect("should write");

    let report = detect_batch_conflicts(&batch, dest.path()).expect("should succeed");

    assert!(report.has_conflicts);
    assert_eq!(report.total_conflicts, 1);
    assert_eq!(report.existing_files.len(), 1);
    assert_eq!(report.existing_files[0], dest.path().join("file1.txt"));

    // Size mismatch should be recorded (10 vs 17 bytes)
    assert_eq!(report.size_mismatches.len(), 1);
    let (path, source_size, dest_size) = &report.size_mismatches[0];
    assert_eq!(path, &dest.path().join("file1.txt"));
    assert_eq!(*source_size, 10);
    assert_eq!(*dest_size, 17); // "different content".len()
}

#[test]
fn test_symlink_conflict_no_size_comparison() {
    let batch = create_test_batch();
    let dest = TempDir::new().expect("temp dir");

    // Create a symlink at destination
    fs::write(dest.path().join("target.txt"), b"target").expect("should write");
    symlink(
        dest.path().join("target.txt"),
        dest.path().join("file1.txt"),
    )
    .expect("should symlink");

    let report = detect_batch_conflicts(&batch, dest.path()).expect("should succeed");

    assert!(report.has_conflicts);
    assert_eq!(report.total_conflicts, 1);
    assert_eq!(report.existing_files.len(), 1);
    assert_eq!(report.existing_files[0], dest.path().join("file1.txt"));

    // Size mismatches should be empty for symlinks
    assert!(report.size_mismatches.is_empty());
}

#[test]
fn test_directory_conflict() {
    let batch = create_test_batch();
    let dest = TempDir::new().expect("temp dir");

    // Create a directory with the same name as a file
    fs::create_dir_all(dest.path().join("file1.txt")).expect("should create dir");

    let report = detect_batch_conflicts(&batch, dest.path()).expect("should succeed");

    assert!(report.has_conflicts);
    assert_eq!(report.total_conflicts, 1);
    assert_eq!(report.existing_files.len(), 1);
    assert_eq!(report.existing_files[0], dest.path().join("file1.txt"));

    // No size comparison for directories
    assert!(report.size_mismatches.is_empty());
}

#[test]
fn test_same_size_no_mismatch() {
    let batch = create_test_batch();
    let dest = TempDir::new().expect("temp dir");

    // Create file with same size
    fs::write(dest.path().join("file1.txt"), b"1234567890").expect("should write"); // 10 bytes

    let report = detect_batch_conflicts(&batch, dest.path()).expect("should succeed");

    assert!(report.has_conflicts);
    assert_eq!(report.total_conflicts, 1);
    assert_eq!(report.existing_files.len(), 1);

    // Size mismatches should be empty because sizes are equal
    assert!(report.size_mismatches.is_empty());
}

#[test]
fn test_destination_not_accessible_continues() {
    let batch = create_test_batch();
    let dest = TempDir::new().expect("temp dir");

    // Remove destination to simulate missing directory
    fs::remove_dir_all(dest.path()).expect("should remove");

    // Should succeed with empty report (destination doesn't exist)
    let report = detect_batch_conflicts(&batch, dest.path()).expect("should succeed");

    assert!(!report.has_conflicts);
    assert_eq!(report.total_conflicts, 0);
}

#[test]
fn test_scans_each_parent_directory_once_for_multiple_files() {
    let batch = Batch {
        id: "scan-parent-once".to_string(),
        files: vec![
            FileEntry {
                relative_path: PathBuf::from("dir/a.txt"),
                size_bytes: 3,
                modified_time: Some(SystemTime::now()),
            },
            FileEntry {
                relative_path: PathBuf::from("dir/b.txt"),
                size_bytes: 3,
                modified_time: Some(SystemTime::now()),
            },
            FileEntry {
                relative_path: PathBuf::from("root.txt"),
                size_bytes: 4,
                modified_time: Some(SystemTime::now()),
            },
        ],
        total_bytes: 10,
        file_count: 3,
    };
    let dest = TempDir::new().expect("temp dir");
    fs::create_dir_all(dest.path().join("dir")).expect("create dir");
    fs::write(dest.path().join("dir/a.txt"), b"aaa").expect("write file");
    fs::write(dest.path().join("root.txt"), b"xxxx").expect("write file");

    let report = detect_batch_conflicts(&batch, dest.path()).expect("should succeed");

    // Parent directories should be scanned once each: destination root + destination/dir.
    assert_eq!(report.scanned_parent_directories, 2);
    assert_eq!(report.total_conflicts, 2);
}

#[test]
fn test_missing_parent_directory_is_scanned_once_even_with_many_files() {
    let batch = Batch {
        id: "missing-parent-once".to_string(),
        files: vec![
            FileEntry {
                relative_path: PathBuf::from("missing/a.txt"),
                size_bytes: 1,
                modified_time: Some(SystemTime::now()),
            },
            FileEntry {
                relative_path: PathBuf::from("missing/b.txt"),
                size_bytes: 1,
                modified_time: Some(SystemTime::now()),
            },
            FileEntry {
                relative_path: PathBuf::from("missing/c.txt"),
                size_bytes: 1,
                modified_time: Some(SystemTime::now()),
            },
        ],
        total_bytes: 3,
        file_count: 3,
    };
    let dest = TempDir::new().expect("temp dir");

    let report = detect_batch_conflicts(&batch, dest.path()).expect("should succeed");

    // `missing/` should only be probed once, not once per file.
    assert_eq!(report.scanned_parent_directories, 1);
    assert_eq!(report.total_conflicts, 0);
}
