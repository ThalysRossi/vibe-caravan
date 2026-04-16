use std::fs;

use assert_cmd::Command;
use caravan::models::state::{BatchPhase, BatchState, MigrationPhase, MigrationState};
use caravan::state_store::{load_state, persist_state};
use tempfile::TempDir;

#[test]
fn resume_skips_already_copied_batches_and_verifies() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    fs::write(source_dir.join("file1.txt"), "aaaa").expect("create source file1");
    fs::write(source_dir.join("file2.txt"), "bbbb").expect("create source file2");
    fs::write(source_dir.join("file3.txt"), "cccc").expect("create source file3");

    fs::write(dest_dir.join("file1.txt"), "aaaa").expect("create destination file1");
    fs::write(dest_dir.join("file2.txt"), "bbbb").expect("create destination file2");

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
    state.upsert_batch(BatchState {
        batch_id: "batch-000003".to_string(),
        phase: BatchPhase::Planned,
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
        "resume should complete when pre-copied batches are valid"
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
        stdout.contains("=== Processing batch-000003"),
        "resume should process copy for planned batch-000003, got: {stdout}"
    );
    assert!(
        !stdout.contains("=== Processing batch-000001"),
        "resume should not recopy batch-000001, got: {stdout}"
    );
    assert!(
        !stdout.contains("=== Processing batch-000002"),
        "resume should not recopy batch-000002, got: {stdout}"
    );

    let state_after = load_state(&state_path).expect("load state after resume");
    for batch_id in ["batch-000001", "batch-000002", "batch-000003"] {
        let batch = state_after.batch(batch_id).expect("batch should exist");
        assert_eq!(batch.phase, BatchPhase::VerifyCompleted);
        assert!(
            batch.verification_passed,
            "{batch_id} should be verified after resume"
        );
    }

    assert_eq!(
        fs::read_to_string(dest_dir.join("file3.txt")).expect("planned batch file should exist"),
        "cccc"
    );
    assert!(source_dir.join("file1.txt").exists());
    assert!(source_dir.join("file2.txt").exists());
    assert!(source_dir.join("file3.txt").exists());
}
