use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

use caravan::models::state::{BatchPhase, BatchState, MigrationState};
use caravan::state_store::{load_state, persist_state};

#[test]
fn resume_aborts_when_failed_batch_requires_operator_review() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "content").expect("create source file");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024 * 1024;
    state.batches.push(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::Failed,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });

    let state_path = tmp.path().join("resume-state.json");
    persist_state(&state_path, &state).expect("persist state");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "resume",
            "--state",
            state_path.to_str().expect("utf8 state path"),
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(
        !output.status.success(),
        "resume should fail closed for batches marked Failed"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("one or more batches require operator review before continuing"),
        "stderr should indicate operator-review gate: {stderr}"
    );

    let state_after = load_state(&state_path).expect("load state");
    let batch = state_after
        .batch("batch-000001")
        .expect("batch should remain present");
    assert_eq!(batch.phase, BatchPhase::Failed);
    assert!(
        !dest_dir.join("file1.txt").exists(),
        "resume should not copy failed batch files before operator review"
    );
}
