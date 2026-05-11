use std::fs;
use std::io::Write;

use caravan::error::CaravanError;
use caravan::models::verification::VerificationStatus;
use caravan::plan::{PlanOptions, build_plan};
use caravan::progress::NoopProgress;
use caravan::transfer::LocalFsCopyBackend;
use caravan::verify::{digest_file, digest_file_with_interrupt, verify_batch_with_progress};
use tempfile::TempDir;

fn create_file(root: &std::path::Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent dirs should be created");
    }
    fs::write(path, bytes).expect("file should be created");
}

fn copy_batch_noop(
    batch: &caravan::models::batch::Batch,
    source_root: &std::path::Path,
    destination_root: &std::path::Path,
) -> Result<(), CaravanError> {
    let mut no_interrupt = || Ok::<(), CaravanError>(());
    LocalFsCopyBackend::new().copy_batch(
        batch,
        source_root,
        destination_root,
        &mut NoopProgress,
        &mut no_interrupt,
    )
}

fn verify_batch_noop(
    batch: &caravan::models::batch::Batch,
    source_root: &std::path::Path,
    destination_root: &std::path::Path,
) -> Result<caravan::models::verification::VerificationReport, CaravanError> {
    let mut no_interrupt = || Ok::<(), CaravanError>(());
    verify_batch_with_progress(
        batch,
        source_root,
        destination_root,
        &mut NoopProgress,
        &mut no_interrupt,
    )
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

    copy_batch_noop(batch, src.path(), dst.path()).expect("copy should succeed");
    let report = verify_batch_noop(batch, src.path(), dst.path()).expect("verify should succeed");

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

    let report = verify_batch_noop(batch, src.path(), dst.path()).expect("verify should succeed");
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
    copy_batch_noop(batch, src.path(), dst.path()).expect("copy should succeed");

    create_file(dst.path(), "bin/data.bin", b"abc");

    let report = verify_batch_noop(batch, src.path(), dst.path()).expect("verify should succeed");
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
    copy_batch_noop(batch, src.path(), dst.path()).expect("copy should succeed");

    create_file(dst.path(), "docs/report.txt", b"same-size-datA");

    let report = verify_batch_noop(batch, src.path(), dst.path()).expect("verify should succeed");
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
    copy_batch_noop(batch, src.path(), dst.path()).expect("copy should succeed");

    fs::remove_file(src.path().join("x/file.txt")).expect("source file removal should succeed");

    let report = verify_batch_noop(batch, src.path(), dst.path()).expect("verify should succeed");
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
    copy_batch_noop(batch, src.path(), dst.path()).expect("copy should succeed");

    let report = verify_batch_noop(batch, src.path(), dst.path()).expect("verify should succeed");
    let json = serde_json::to_string(&report).expect("report should serialize");
    assert!(json.contains("\"status\":\"Pass\""));
}

#[test]
fn streaming_hash_produces_same_result_as_full_read() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("test.bin");

    // Create test file with content larger than 1MB buffer
    // This test would cause stack overflow on Windows with stack-allocated buffer
    // due to Windows' default 1MB thread stack size. Heap allocation prevents this.
    let mut file = fs::File::create(&path).expect("create file");
    let block = b"test_pattern_1234567890";
    for _ in 0..150000 {
        // ~3.45MB file
        file.write_all(block).expect("write block");
    }
    drop(file);

    // Get hash from streaming implementation
    let streaming = digest_file(&path).expect("streaming hash should succeed");

    // Verify against blake3 direct hash of full file
    let full = fs::read(&path).expect("read full file");
    let expected = blake3::hash(&full);

    assert_eq!(
        streaming,
        *expected.as_bytes(),
        "Streaming hash must match full file hash"
    );
}

#[test]
fn very_large_file_hash_without_stack_overflow() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("very_large.bin");

    // Create a 10MB file to stress test the heap-allocated buffer
    // This would definitely cause stack overflow on Windows with stack allocation
    let mut file = fs::File::create(&path).expect("create file");
    let block = vec![0x42u8; 1024 * 1024]; // 1MB block
    for _ in 0..10 {
        // 10MB total
        file.write_all(&block).expect("write block");
    }
    drop(file);

    let hash = digest_file(&path).expect("very large file hash should succeed");
    let full = fs::read(&path).expect("read full file");
    let expected = blake3::hash(&full);

    assert_eq!(
        hash,
        *expected.as_bytes(),
        "Very large file hash must match"
    );
}

#[test]
fn empty_file_streaming_hash() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("empty.bin");

    fs::File::create(&path).expect("create empty file");

    let hash = digest_file(&path).expect("empty file hash should succeed");
    let expected = blake3::hash(&[]);

    assert_eq!(hash, *expected.as_bytes());
}

#[test]
fn single_byte_file_streaming_hash() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("single.bin");

    fs::write(&path, b"X").expect("write single byte");

    let hash = digest_file(&path).expect("single byte hash should succeed");
    let expected = blake3::hash(b"X");

    assert_eq!(hash, *expected.as_bytes());
}

#[test]
fn exact_buffer_size_file_hash() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("exact.bin");

    let data = vec![0xAA; 1024 * 1024]; // Exactly 1MB buffer size
    fs::write(&path, &data).expect("write exact buffer size");

    let hash = digest_file(&path).expect("exact size hash should succeed");
    let expected = blake3::hash(&data);

    assert_eq!(hash, *expected.as_bytes());
}

#[test]
fn digest_file_with_interrupt_returns_graceful_shutdown() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("interrupt.bin");
    fs::write(&path, b"content").expect("write file");
    let mut check_interrupt = || Err(CaravanError::GracefulShutdown);

    let err = digest_file_with_interrupt(&path, &mut check_interrupt)
        .expect_err("digest should stop when shutdown is requested");

    assert!(matches!(err, CaravanError::GracefulShutdown));
}
