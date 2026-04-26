use std::fs;
use std::path::Path;

use caravan::transfer::{FileCopier, OsFileCopier};
use tempfile::TempDir;

// Helper function to create a test file with specific content
fn create_test_file(path: &Path, content: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("should create parent directory");
    }
    fs::write(path, content).expect("should write test file");
}

// Helper function to read file content
fn read_file_content(path: &Path) -> Vec<u8> {
    fs::read(path).expect("should read file")
}

#[test]
fn os_file_copier_works_with_small_file() {
    let temp_dir = TempDir::new().expect("should create temp dir");
    let source = temp_dir.path().join("source.txt");
    let destination = temp_dir.path().join("dest.txt");

    let content = b"Hello, world!";
    create_test_file(&source, content);

    let copier = OsFileCopier;
    let bytes_copied = copier
        .copy_file(&source, &destination)
        .expect("copy should succeed");

    assert_eq!(bytes_copied, content.len() as u64);
    assert_eq!(read_file_content(&destination), content);
}

#[test]
fn os_file_copier_handles_empty_file() {
    let temp_dir = TempDir::new().expect("should create temp dir");
    let source = temp_dir.path().join("empty.txt");
    let destination = temp_dir.path().join("empty_copy.txt");

    create_test_file(&source, b"");

    let copier = OsFileCopier;
    let bytes_copied = copier
        .copy_file(&source, &destination)
        .expect("copy should succeed");

    assert_eq!(bytes_copied, 0);
    assert!(destination.exists());
    assert_eq!(read_file_content(&destination).len(), 0);
}

#[test]
fn os_file_copier_returns_error_for_nonexistent_source() {
    let temp_dir = TempDir::new().expect("should create temp dir");
    let source = temp_dir.path().join("nonexistent.txt");
    let destination = temp_dir.path().join("dest.txt");

    let copier = OsFileCopier;
    let result = copier.copy_file(&source, &destination);

    assert!(result.is_err());
    assert!(!destination.exists());
}
