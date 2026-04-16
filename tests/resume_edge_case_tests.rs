use std::fs;

use assert_cmd::Command;
use caravan::models::state::{BatchPhase, BatchState, MigrationState};
use caravan::state_store::persist_state;
use tempfile::TempDir;

/// Resume should return a typed error when a persisted batch ID cannot be reconstructed.
#[test]
fn resume_handles_missing_batch_gracefully() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "content").expect("create source file");

    let state_path = tmp.path().join("resume-state.json");
    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024;
    state.upsert_batch(BatchState {
        batch_id: "batch-000999".to_string(),
        phase: BatchPhase::Planned,
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
        "resume should fail cleanly when batch id cannot be loaded"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Could not locate batch batch-000999 in source directory"),
        "expected explicit missing-batch error, got: {stderr}"
    );
}

/// Test that batch lookup returns Option rather than panicking.
#[test]
fn batch_lookup_returns_option() {
    let state = MigrationState::new("staging", "/source", "/dest");

    let result = state.batch("non-existent-batch");
    assert!(result.is_none());

    let mut state_mut = MigrationState::new("staging", "/source", "/dest");
    let result_mut = state_mut.batch_mut("non-existent-batch");
    assert!(result_mut.is_none());
}
