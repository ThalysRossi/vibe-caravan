use std::fs;
use std::path::Path;

use caravan::transfer::{BufferedFileCopier, FileCopier, OsFileCopier};
use tempfile::TempDir;

#[cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;

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

#[test]
fn buffered_copy_creates_new_test_file() {
    let temp_dir = TempDir::new().expect("should create temp dir");
    let source = temp_dir.path().join("source.txt");
    let destination = temp_dir.path().join("dest.txt");

    let content = b"Test content for buffered copy";
    create_test_file(&source, content);

    let copier = BufferedFileCopier::default();
    let bytes_copied = copier
        .copy_file(&source, &destination)
        .expect("buffered copy should succeed");

    assert_eq!(bytes_copied, content.len() as u64);
    assert!(destination.exists());
    assert_eq!(read_file_content(&destination), content);
}

#[test]
fn buffered_copy_copies_exact_bytes() {
    let temp_dir = TempDir::new().expect("should create temp dir");
    let source = temp_dir.path().join("source.bin");
    let destination = temp_dir.path().join("dest.bin");

    // Create a file larger than default buffer size to test chunking
    let content = vec![42u8; BufferedFileCopier::DEFAULT_BUFFER_SIZE * 2];
    create_test_file(&source, &content);

    let copier = BufferedFileCopier::default();
    let bytes_copied = copier
        .copy_file(&source, &destination)
        .expect("buffered copy should succeed");

    assert_eq!(bytes_copied, content.len() as u64);
    assert_eq!(read_file_content(&destination), content);
}

#[test]
fn buffered_copy_preserves_file_contents() {
    let temp_dir = TempDir::new().expect("should create temp dir");
    let source = temp_dir.path().join("data.txt");
    let destination = temp_dir.path().join("copy.txt");

    // Create file with varied content (using u32 for consistent size)
    let mut content = Vec::new();
    for i in 0u32..1000 {
        content.extend_from_slice(&i.to_le_bytes());
    }
    create_test_file(&source, &content);

    let copier = BufferedFileCopier::new(1024); // Use small buffer for testing
    let bytes_copied = copier
        .copy_file(&source, &destination)
        .expect("buffered copy should succeed");

    assert_eq!(bytes_copied, content.len() as u64);
    assert_eq!(read_file_content(&destination), content);
}

#[test]
fn buffered_copy_handles_empty_files() {
    let temp_dir = TempDir::new().expect("should create temp dir");
    let source = temp_dir.path().join("empty.txt");
    let destination = temp_dir.path().join("empty_copy.txt");

    create_test_file(&source, b"");

    let copier = BufferedFileCopier::default();
    let bytes_copied = copier
        .copy_file(&source, &destination)
        .expect("buffered copy should succeed");

    assert_eq!(bytes_copied, 0);
    assert!(destination.exists());
    assert_eq!(read_file_content(&destination).len(), 0);
}

#[test]
fn buffered_copy_returns_correct_byte_count() {
    let temp_dir = TempDir::new().expect("should create temp dir");
    let source = temp_dir.path().join("test.txt");
    let destination = temp_dir.path().join("test_copy.txt");

    // File size that's not a multiple of buffer size
    let content = vec![1u8; 12345];
    create_test_file(&source, &content);

    let copier = BufferedFileCopier::new(4096); // 4KB buffer
    let bytes_copied = copier
        .copy_file(&source, &destination)
        .expect("buffered copy should succeed");

    assert_eq!(bytes_copied, 12345);
    assert_eq!(read_file_content(&destination).len(), 12345);
}

#[test]
fn buffered_copy_with_custom_buffer_size() {
    let temp_dir = TempDir::new().expect("should create temp dir");
    let source = temp_dir.path().join("custom.txt");
    let destination = temp_dir.path().join("custom_copy.txt");

    let content = b"Test with custom buffer size";
    create_test_file(&source, content);

    // Use very small buffer size to ensure multiple read/write cycles
    let copier = BufferedFileCopier::new(8); // 8 byte buffer
    let bytes_copied = copier
        .copy_file(&source, &destination)
        .expect("buffered copy should succeed");

    assert_eq!(bytes_copied, content.len() as u64);
    assert_eq!(read_file_content(&destination), content);
}

#[test]
fn buffered_copy_returns_error_for_nonexistent_source() {
    let temp_dir = TempDir::new().expect("should create temp dir");
    let source = temp_dir.path().join("nonexistent.txt");
    let destination = temp_dir.path().join("dest.txt");

    let copier = BufferedFileCopier::default();
    let result = copier.copy_file(&source, &destination);

    assert!(result.is_err());
    assert!(!destination.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn buffered_copy_preserves_unix_permissions() {
    let temp_dir = TempDir::new().expect("should create temp dir");
    let source = temp_dir.path().join("source.sh");
    let destination = temp_dir.path().join("dest.sh");

    create_test_file(&source, b"#!/bin/sh\necho test\n");
    fs::set_permissions(&source, fs::Permissions::from_mode(0o751))
        .expect("should set source permissions");

    let copier = BufferedFileCopier::default();
    copier
        .copy_file(&source, &destination)
        .expect("buffered copy should succeed");

    let source_mode = fs::metadata(&source)
        .expect("source metadata")
        .permissions()
        .mode()
        & 0o777;
    let dest_mode = fs::metadata(&destination)
        .expect("destination metadata")
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(dest_mode, source_mode);
}
