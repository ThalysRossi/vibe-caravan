use caravan::models::batch::Batch;
use caravan::models::file_entry::FileEntry;
use caravan::progress::NoopProgress;
use caravan::verify::verify_batch_with_progress;
use std::fs;
use tempfile::TempDir;

fn verify_batch_noop(
    batch: &Batch,
    source_root: &std::path::Path,
    destination_root: &std::path::Path,
) -> Result<caravan::models::verification::VerificationReport, caravan::error::CaravanError> {
    let mut no_interrupt = || Ok::<(), caravan::error::CaravanError>(());
    verify_batch_with_progress(
        batch,
        source_root,
        destination_root,
        &mut NoopProgress,
        &mut no_interrupt,
    )
}

#[test]
fn verification_skips_caravan_files() {
    let src = TempDir::new().expect("source temp dir");
    let dst = TempDir::new().expect("destination temp dir");

    // Create regular file
    fs::write(src.path().join("file.txt"), "content").expect("create file");
    fs::write(dst.path().join("file.txt"), "content").expect("create copy");

    // Create .caravan directory with different content in source vs dest
    let src_caravan = src.path().join(".caravan");
    let dst_caravan = dst.path().join(".caravan");
    fs::create_dir_all(&src_caravan).expect("create src .caravan");
    fs::create_dir_all(&dst_caravan).expect("create dst .caravan");

    // Source state file (will be updated during migration)
    fs::write(
        src_caravan.join("state.json"),
        r#"{"mode":"staging","updated":1}"#,
    )
    .expect("create src state");
    // Destination state file (copied earlier, might be older version)
    fs::write(
        dst_caravan.join("state.json"),
        r#"{"mode":"staging","updated":0}"#,
    )
    .expect("create dst state");

    // Create a batch that includes .caravan files (simulating old scan)
    let files = vec![
        FileEntry {
            relative_path: "file.txt".into(),
            size_bytes: 7,
            modified_time: None,
        },
        FileEntry {
            relative_path: ".caravan/state.json".into(),
            size_bytes: 33,
            modified_time: None,
        },
    ];
    let total_bytes = files.iter().map(|f| f.size_bytes).sum();
    let batch = Batch {
        id: "batch-000001".to_string(),
        files,
        total_bytes,
        file_count: 2,
    };

    // Verification should pass because .caravan files are skipped
    let report = verify_batch_noop(&batch, src.path(), dst.path()).expect("verify should succeed");

    // Should pass despite .caravan/state.json mismatch
    assert_eq!(
        report.status,
        caravan::models::verification::VerificationStatus::Pass,
        "Verification should pass when skipping .caravan files"
    );
    assert!(
        report.missing_files.is_empty(),
        "No files should be missing"
    );
    assert!(
        report.mismatched_files.is_empty(),
        "No files should be mismatched (including .caravan)"
    );
}

#[test]
fn verification_skips_caravan_files_in_subdirectories() {
    let src = TempDir::new().expect("source temp dir");
    let dst = TempDir::new().expect("destination temp dir");

    // Create parent directories for regular file
    fs::create_dir_all(src.path().join("docs")).expect("create src docs dir");
    fs::create_dir_all(dst.path().join("docs")).expect("create dst docs dir");
    fs::write(src.path().join("docs/readme.txt"), "readme").expect("create file");
    fs::write(dst.path().join("docs/readme.txt"), "readme").expect("create copy");

    // Create .caravan in subdirectory
    let src_caravan = src.path().join("docs/.caravan");
    let dst_caravan = dst.path().join("docs/.caravan");
    fs::create_dir_all(&src_caravan).expect("create src docs/.caravan");
    fs::create_dir_all(&dst_caravan).expect("create dst docs/.caravan");

    // Different cache files
    fs::write(src_caravan.join("cache.json"), "new").expect("create src cache");
    fs::write(dst_caravan.join("cache.json"), "old").expect("create dst cache");

    let files = vec![
        FileEntry {
            relative_path: "docs/readme.txt".into(),
            size_bytes: 6,
            modified_time: None,
        },
        FileEntry {
            relative_path: "docs/.caravan/cache.json".into(),
            size_bytes: 3,
            modified_time: None,
        },
    ];
    let total_bytes = files.iter().map(|f| f.size_bytes).sum();
    let batch = Batch {
        id: "batch-000001".to_string(),
        files,
        total_bytes,
        file_count: 2,
    };

    // Verification should pass (skip .caravan file)
    let report = verify_batch_noop(&batch, src.path(), dst.path()).expect("verify should succeed");

    assert_eq!(
        report.status,
        caravan::models::verification::VerificationStatus::Pass,
        "Verification should pass when skipping nested .caravan files"
    );
    assert!(
        report.mismatched_files.is_empty(),
        "No mismatched files should be reported (including nested .caravan)"
    );
}
