use std::fs;

use assert_cmd::Command;
use caravan::models::state::{BatchPhase, BatchState, MigrationPhase, MigrationState};
use caravan::state_store::{load_state, persist_state};
use tempfile::TempDir;

#[test]
fn resume_command_skips_copy_for_copy_completed_batch_and_completes() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    fs::write(source_dir.join("test.txt"), "test").expect("write source file");
    fs::write(dest_dir.join("test.txt"), "test").expect("write destination file");

    let state_path = tmp.path().join("resume-state.json");
    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024;
    state.migration_phase = MigrationPhase::Copying;
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::CopyCompleted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });
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
        "resume should complete when destination is ready and interactive deletion is enabled"
    );

    let state_after = load_state(&state_path).expect("load state after resume");
    let batch = state_after
        .batch("batch-000001")
        .expect("batch should remain present");
    assert_eq!(batch.phase, BatchPhase::VerifyCompleted);
    assert!(batch.verification_passed);
    assert!(!batch.approved_for_delete);
    assert!(!batch.deleted);
    assert!(source_dir.join("test.txt").exists());
}

#[test]
fn resume_command_blocks_copy_completed_batch_when_destination_is_missing() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    fs::write(source_dir.join("test.txt"), "test").expect("write source file");

    let state_path = tmp.path().join("resume-state.json");
    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024;
    state.migration_phase = MigrationPhase::Copying;
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::CopyCompleted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });
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
        "operator-review reason should include destination diff, got: {stderr}"
    );
    assert!(
        stderr.contains("test.txt"),
        "operator-review reason should include the missing file path, got: {stderr}"
    );

    let state_after = load_state(&state_path).expect("load state after failed resume");
    let batch = state_after
        .batch("batch-000001")
        .expect("batch should remain present");
    assert_eq!(batch.phase, BatchPhase::CopyCompleted);
    assert!(!batch.verification_passed);
    assert!(!dest_dir.join("test.txt").exists());
}

#[test]
fn resume_rejects_invalid_copy_buffer_size_loaded_from_state() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    let state_path = tmp.path().join("resume-state.json");
    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024;
    state.copy_buffer_size = 0;
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
        "resume should reject zero copy buffer size from persisted state"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("copy-buffer-size: size must be greater than zero"),
        "resume should surface copy option validation error, got: {stderr}"
    );
}

#[test]
fn resume_rejects_invalid_buffered_copy_threshold_loaded_from_state() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    let state_path = tmp.path().join("resume-state.json");
    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024;
    state.buffered_copy_threshold = 0;
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
        "resume should reject zero buffered copy threshold from persisted state"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("buffered-copy-threshold: size must be greater than zero"),
        "resume should surface buffered threshold validation error, got: {stderr}"
    );
}
