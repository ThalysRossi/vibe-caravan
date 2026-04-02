use std::fs;

use tempfile::TempDir;
use wololo::config::VerificationMode;
use wololo::models::verification::VerificationStatus;
use wololo::plan::{build_plan, PlanOptions};
use wololo::transfer::{transfer_batch, LocalFsCopyBackend};
use wololo::verify::verify_batch;

fn create_file(root: &std::path::Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent dirs should be created");
    }
    fs::write(path, bytes).expect("file should be created");
}

#[test]
fn copied_file_contents_match_source() {
    let src = TempDir::new().expect("source temp dir");
    let dst = TempDir::new().expect("destination temp dir");
    create_file(src.path(), "docs/a.txt", b"hello world");

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
    let report =
        verify_batch(batch, src.path(), dst.path(), VerificationMode::Digest).expect("verify should succeed");

    assert_eq!(report.status, VerificationStatus::Pass);
}

#[test]
fn missing_files_fail_verification() {
    let src = TempDir::new().expect("source temp dir");
    let dst = TempDir::new().expect("destination temp dir");
    create_file(src.path(), "photos/1.jpg", b"123456");

    let plan = build_plan(
        src.path(),
        &PlanOptions {
            batch_size_bytes: 1024,
            max_files: Some(10),
        },
    )
    .expect("planning should succeed");
    let batch = &plan.batches[0];

    let report =
        verify_batch(batch, src.path(), dst.path(), VerificationMode::Digest).expect("verify should succeed");
    assert_eq!(report.status, VerificationStatus::Fail);
    assert_eq!(report.missing_files.len(), 1);
}

#[test]
fn size_mismatch_fails_verification() {
    let src = TempDir::new().expect("source temp dir");
    let dst = TempDir::new().expect("destination temp dir");
    create_file(src.path(), "bin/data.bin", b"abcdefghij");

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

    create_file(dst.path(), "bin/data.bin", b"abc");

    let report =
        verify_batch(batch, src.path(), dst.path(), VerificationMode::Digest).expect("verify should succeed");
    assert_eq!(report.status, VerificationStatus::Fail);
    assert_eq!(report.mismatched_files.len(), 1);
}

#[test]
fn digest_mismatch_fails_verification() {
    let src = TempDir::new().expect("source temp dir");
    let dst = TempDir::new().expect("destination temp dir");
    create_file(src.path(), "docs/report.txt", b"same-size-data");

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

    create_file(dst.path(), "docs/report.txt", b"same-size-datA");

    let report =
        verify_batch(batch, src.path(), dst.path(), VerificationMode::Digest).expect("verify should succeed");
    assert_eq!(report.status, VerificationStatus::Fail);
    assert_eq!(report.mismatched_files.len(), 1);
}

#[test]
fn unreadable_file_fails_verification() {
    let src = TempDir::new().expect("source temp dir");
    let dst = TempDir::new().expect("destination temp dir");
    create_file(src.path(), "x/file.txt", b"content");

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

    fs::remove_file(src.path().join("x/file.txt")).expect("source file removal should succeed");

    let report =
        verify_batch(batch, src.path(), dst.path(), VerificationMode::Digest).expect("verify should succeed");
    assert_eq!(report.status, VerificationStatus::Fail);
    assert_eq!(report.unreadable_files.len(), 1);
}

#[test]
fn verification_report_serializes_to_json() {
    let src = TempDir::new().expect("source temp dir");
    let dst = TempDir::new().expect("destination temp dir");
    create_file(src.path(), "ok.txt", b"ok");

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

    let report =
        verify_batch(batch, src.path(), dst.path(), VerificationMode::Digest).expect("verify should succeed");
    let json = serde_json::to_string(&report).expect("report should serialize");
    assert!(json.contains("\"status\":\"Pass\""));
}
