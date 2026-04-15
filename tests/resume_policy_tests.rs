use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

use caravan::models::state::{BatchPhase, BatchState, MigrationState};
use caravan::state_store::{load_state, persist_state};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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

#[test]
fn resume_aborts_when_state_contains_failed_verification_batch() {
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
        phase: BatchPhase::VerifyCompleted,
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
        "resume should fail closed for failed verification state"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("one or more batches failed verification"),
        "stderr should indicate failed-verification gate: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn resume_does_not_recopy_copy_started_batch_when_destination_is_complete() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    fs::write(source_dir.join("file1.txt"), "content").expect("create source file");
    fs::write(dest_dir.join("file1.txt"), "content").expect("create destination file");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024 * 1024;
    state.batches.push(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::CopyStarted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });

    let state_path = tmp.path().join("resume-state.json");
    persist_state(&state_path, &state).expect("persist state");

    let file_perm = fs::Permissions::from_mode(0o444);
    fs::set_permissions(dest_dir.join("file1.txt"), file_perm).expect("set file readonly");
    let dir_perm = fs::Permissions::from_mode(0o555);
    fs::set_permissions(&dest_dir, dir_perm).expect("set dir readonly");

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

    // Restore permissions so TempDir cleanup succeeds.
    fs::set_permissions(&dest_dir, fs::Permissions::from_mode(0o755))
        .expect("restore dir permissions");
    fs::set_permissions(
        dest_dir.join("file1.txt"),
        fs::Permissions::from_mode(0o644),
    )
    .expect("restore file permissions");

    assert!(
        output.status.success(),
        "resume should verify an already-complete CopyStarted batch without recopying"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("failed to copy"),
        "resume should not attempt a copy when destination reconciliation is complete: {stderr}"
    );
}

#[test]
fn resume_deletes_batches_already_approved_for_delete() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    fs::write(source_dir.join("file1.txt"), "content").expect("create source file");
    fs::write(dest_dir.join("file1.txt"), "content").expect("create destination file");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024 * 1024;
    state.batches.push(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::ApprovedForDelete,
        verification_passed: true,
        approved_for_delete: true,
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
        output.status.success(),
        "resume should complete approved-for-delete batches"
    );
    assert!(
        !source_dir.join("file1.txt").exists(),
        "source file should be deleted for approved-for-delete batches"
    );
}
