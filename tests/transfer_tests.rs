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

    transfer_batch(batch, src.path(), dst.path(), &LocalFsCopyBackend).expect("copy should succeed");

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

    let backend = LocalFsCopyBackend;
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

    let err = transfer_batch(batch, src.path(), dst.path(), &LocalFsCopyBackend)
        .expect_err("copy should fail for missing source");
    assert!(err.to_string().contains("failed to copy"));
}
