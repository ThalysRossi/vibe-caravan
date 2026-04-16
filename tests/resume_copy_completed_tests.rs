use std::fs;

use assert_cmd::Command;
use caravan::models::state::{BatchPhase, BatchState, MigrationPhase, MigrationState};
use caravan::state_store::{load_state, persist_state};
use tempfile::TempDir;

#[test]
fn resume_with_copy_completed_batches_should_verify_not_copy() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    fs::write(source_dir.join("file1.txt"), "aaaa").expect("create source file1");
    fs::write(source_dir.join("file2.txt"), "bbbb").expect("create source file2");
    fs::write(dest_dir.join("file1.txt"), "aaaa").expect("create dest file1");
    fs::write(dest_dir.join("file2.txt"), "bbbb").expect("create dest file2");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 4;
    state.migration_phase = MigrationPhase::Copying;
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::CopyCompleted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });
    state.upsert_batch(BatchState {
        batch_id: "batch-000002".to_string(),
        phase: BatchPhase::CopyCompleted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });

    let state_path = tmp.path().join("resume-state.json");
    persist_state(&state_path, &state).expect("persist state");

    let binary = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(binary)
        .args([
            "resume",
            "--state",
            state_path.to_str().expect("utf8 state path"),
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute resume");

    assert!(
        output.status.success(),
        "resume should verify CopyCompleted batches without recopying"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Verifying batch-000001"),
        "resume should verify batch-000001, got: {stdout}"
    );
    assert!(
        stdout.contains("Verifying batch-000002"),
        "resume should verify batch-000002, got: {stdout}"
    );
    assert!(
        !stdout.contains("=== Processing batch-000001"),
        "resume should skip copy step for batch-000001, got: {stdout}"
    );
    assert!(
        !stdout.contains("=== Processing batch-000002"),
        "resume should skip copy step for batch-000002, got: {stdout}"
    );

    let state_after = load_state(&state_path).expect("load state after resume");
    let batch1 = state_after
        .batch("batch-000001")
        .expect("batch-000001 should exist");
    let batch2 = state_after
        .batch("batch-000002")
        .expect("batch-000002 should exist");
    assert_eq!(batch1.phase, BatchPhase::VerifyCompleted);
    assert!(batch1.verification_passed);
    assert_eq!(batch2.phase, BatchPhase::VerifyCompleted);
    assert!(batch2.verification_passed);
    assert!(source_dir.join("file1.txt").exists());
    assert!(source_dir.join("file2.txt").exists());
}

#[test]
fn resume_with_copy_completed_but_missing_files_should_require_review() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    fs::write(source_dir.join("file1.txt"), "aaaa").expect("create source file");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 4;
    state.migration_phase = MigrationPhase::Copying;
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::CopyCompleted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });

    let state_path = tmp.path().join("resume-state.json");
    persist_state(&state_path, &state).expect("persist state");

    let binary = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(binary)
        .args([
            "resume",
            "--state",
            state_path.to_str().expect("utf8 state path"),
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute resume");

    assert!(
        !output.status.success(),
        "resume should fail closed when CopyCompleted state diverges from destination"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("state says copy completed but destination is incomplete"),
        "resume should include reconciliation reason, got: {stderr}"
    );
    assert!(
        stderr.contains("file1.txt"),
        "resume should include missing relative file path, got: {stderr}"
    );

    let state_after = load_state(&state_path).expect("load state after failed resume");
    let batch = state_after
        .batch("batch-000001")
        .expect("batch-000001 should exist");
    assert_eq!(batch.phase, BatchPhase::CopyCompleted);
    assert!(!batch.verification_passed);
}
