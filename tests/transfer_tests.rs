use std::fs;

use tempfile::TempDir;
use caravan::plan::{build_plan, PlanOptions};
use caravan::transfer::{transfer_batch, CopyBackend, LocalFsCopyBackend};

fn create_file(root: &std::path::Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent dirs should be created");
    }
    fs::write(path, bytes).expect("file should be created");
}

#[test]
fn transfer_batch_copies_multiple_files_with_nested_paths() {
    let src = TempDir::new().expect("source temp dir");
    let dst = TempDir::new().expect("destination temp dir");
    create_file(src.path(), "a/one.txt", b"one");
    create_file(src.path(), "b/nested/two.txt", b"two");

    let plan = build_plan(
        src.path(),
        &PlanOptions {
            batch_size_bytes: 1024,
            max_files: Some(10),
        },
    )
    .expect("planning should succeed");
    let batch = &plan.batches[0];

    transfer_batch(batch, src.path(), dst.path(), &LocalFsCopyBackend::new()).expect("copy should succeed");

    assert_eq!(
        fs::read(dst.path().join("a/one.txt")).expect("first copied file should exist"),
        b"one"
    );
    assert_eq!(
        fs::read(dst.path().join("b/nested/two.txt")).expect("second copied file should exist"),
        b"two"
    );
}

#[test]
fn backend_copy_batch_direct_call_is_successful() {
    let src = TempDir::new().expect("source temp dir");
    let dst = TempDir::new().expect("destination temp dir");
    create_file(src.path(), "docs/report.txt", b"report");

    let plan = build_plan(
        src.path(),
        &PlanOptions {
            batch_size_bytes: 1024,
            max_files: Some(10),
        },
    )
    .expect("planning should succeed");
    let batch = &plan.batches[0];

    let backend = LocalFsCopyBackend::new();
    backend
        .copy_batch(batch, src.path(), dst.path())
        .expect("direct backend copy should succeed");

    assert_eq!(
        fs::read(dst.path().join("docs/report.txt")).expect("copied report should exist"),
        b"report"
    );
}

#[test]
fn transfer_batch_returns_error_when_source_file_is_missing() {
    let src = TempDir::new().expect("source temp dir");
    let dst = TempDir::new().expect("destination temp dir");
    create_file(src.path(), "x/data.bin", b"1234");

    let plan = build_plan(
        src.path(),
        &PlanOptions {
            batch_size_bytes: 1024,
            max_files: Some(10),
        },
    )
    .expect("planning should succeed");
    let batch = &plan.batches[0];

    fs::remove_file(src.path().join("x/data.bin")).expect("source file removal should succeed");

    let err = transfer_batch(batch, src.path(), dst.path(), &LocalFsCopyBackend::new())
        .expect_err("copy should fail for missing source");
    assert!(err.to_string().contains("failed to copy"));
}

#[test]
fn directory_creation_deduplicated_for_files_in_same_directory() {
    let src = TempDir::new().expect("source temp dir");
    let dst = TempDir::new().expect("destination temp dir");
    
    // Create 10 files all in the same directory
    for i in 0..10 {
        create_file(src.path(), &format!("data/file{}.txt", i), b"content");
    }
    
    let plan = build_plan(
        src.path(),
        &PlanOptions {
            batch_size_bytes: 1024,
            max_files: Some(20),
        },
    )
    .expect("planning should succeed");
    let batch = &plan.batches[0];
    
    // This should work correctly with deduplication
    transfer_batch(batch, src.path(), dst.path(), &LocalFsCopyBackend::new())
        .expect("copy should succeed with multiple files in same directory");
    
    // Verify all files were copied
    for i in 0..10 {
        let content = fs::read(dst.path().join(format!("data/file{}.txt", i)))
            .unwrap_or_else(|_| panic!("file {} should exist", i));
        assert_eq!(content, b"content");
    }
    
    // Verify the directory exists
    assert!(dst.path().join("data").exists());
    assert!(dst.path().join("data").is_dir());
}

#[test]
fn nested_directories_created_correctly_with_deduplication() {
    let src = TempDir::new().expect("source temp dir");
    let dst = TempDir::new().expect("destination temp dir");
    
    // Create files in nested directory structure
    create_file(src.path(), "a/b/c/deep.txt", b"deep");
    create_file(src.path(), "a/b/middle.txt", b"middle");
    create_file(src.path(), "a/top.txt", b"top");
    create_file(src.path(), "root.txt", b"root");
    
    let plan = build_plan(
        src.path(),
        &PlanOptions {
            batch_size_bytes: 1024,
            max_files: Some(10),
        },
    )
    .expect("planning should succeed");
    let batch = &plan.batches[0];
    
    transfer_batch(batch, src.path(), dst.path(), &LocalFsCopyBackend::new())
        .expect("copy should succeed with nested directories");
    
    // Verify all files and directories
    assert_eq!(fs::read(dst.path().join("a/b/c/deep.txt")).unwrap(), b"deep");
    assert_eq!(fs::read(dst.path().join("a/b/middle.txt")).unwrap(), b"middle");
    assert_eq!(fs::read(dst.path().join("a/top.txt")).unwrap(), b"top");
    assert_eq!(fs::read(dst.path().join("root.txt")).unwrap(), b"root");
    
    // Verify directories exist
    assert!(dst.path().join("a").is_dir());
    assert!(dst.path().join("a/b").is_dir());
    assert!(dst.path().join("a/b/c").is_dir());
}

#[test]
fn files_at_root_level_need_no_directory_creation() {
    let src = TempDir::new().expect("source temp dir");
    let dst = TempDir::new().expect("destination temp dir");
    
    // Create files directly at root (no parent directories needed)
    create_file(src.path(), "file1.txt", b"one");
    create_file(src.path(), "file2.txt", b"two");
    
    let plan = build_plan(
        src.path(),
        &PlanOptions {
            batch_size_bytes: 1024,
            max_files: Some(10),
        },
    )
    .expect("planning should succeed");
    let batch = &plan.batches[0];
    
    transfer_batch(batch, src.path(), dst.path(), &LocalFsCopyBackend::new())
        .expect("copy should succeed for root-level files");
    
    assert_eq!(fs::read(dst.path().join("file1.txt")).unwrap(), b"one");
    assert_eq!(fs::read(dst.path().join("file2.txt")).unwrap(), b"two");
}

#[test]
fn error_message_includes_file_path_when_directory_creation_fails() {
    // Skip on Windows due to complex permission handling
    #[cfg(windows)]
    {
        return;
    }
    
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        
        let src = TempDir::new().expect("source temp dir");
        let dst = TempDir::new().expect("destination temp dir");
        
        // Create a file that will require directory creation
        create_file(src.path(), "subdir/file.txt", b"content");
        
        let plan = build_plan(
            src.path(),
            &PlanOptions {
                batch_size_bytes: 1024,
                max_files: Some(10),
            },
        )
        .expect("planning should succeed");
        let batch = &plan.batches[0];
        
        // Make destination read-only to cause permission error
        let mut perms = fs::metadata(dst.path()).unwrap().permissions();
        perms.set_mode(0o555); // Read and execute only, no write
        fs::set_permissions(dst.path(), perms).unwrap();
        
        let err = transfer_batch(batch, src.path(), dst.path(), &LocalFsCopyBackend::new())
            .expect_err("copy should fail due to permission error");
        
        // Error message should include the file path
        let err_str = err.to_string();
        assert!(err_str.contains("subdir/file.txt") || err_str.contains("while processing"));
        
        // Restore permissions for cleanup
        let mut perms = fs::metadata(dst.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(dst.path(), perms).unwrap();
    }
}
