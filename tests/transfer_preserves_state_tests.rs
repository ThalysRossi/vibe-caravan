use std::fs;

use assert_cmd::Command;
use caravan::migration_registry;
use caravan::models::state::{BatchPhase, BatchState, MigrationPhase, MigrationState};
use caravan::state_store::{load_state, persist_state};
use tempfile::TempDir;

mod common;

use common::{batch_state, migration_state_for_fixture, run_staging, source_dest_fixture};

#[test]
fn transfer_rerun_preserves_copy_completed_batch_and_blocks_on_mismatch() {
    let fixture = source_dest_fixture();
    let source_dir = &fixture.source_dir;
    let dest_dir = &fixture.dest_dir;

    fs::write(source_dir.join("file1.txt"), "aaaa").expect("write source file1");
    fs::write(source_dir.join("file2.txt"), "bbbb").expect("write source file2");
    fs::write(dest_dir.join("file1.txt"), "zzzz").expect("write mismatched destination file1");

    let mut state =
        migration_state_for_fixture("staging", source_dir, dest_dir, 4, MigrationPhase::Copying);
    state.upsert_batch(batch_state(
        "batch-000001",
        BatchPhase::CopyCompleted,
        false,
        false,
        false,
    ));
    state.upsert_batch(batch_state(
        "batch-000002",
        BatchPhase::Planned,
        false,
        false,
        false,
    ));

    let state_path = migration_registry::state_file_in_source(source_dir, dest_dir);
    persist_state(&state_path, &state).expect("persist state");

    let output = run_staging(source_dir, dest_dir, "4B", fixture.tmp.path());

    assert!(
        !output.status.success(),
        "rerun should fail verification when pre-copied batch contents mismatch"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("verification failed"),
        "expected verification failure, got: {stderr}"
    );

    let state_after = load_state(&state_path).expect("load state after rerun");
    let batch1 = state_after
        .batch("batch-000001")
        .expect("batch-000001 should exist");
    assert_eq!(batch1.phase, BatchPhase::VerifyCompleted);
    assert!(!batch1.verification_passed);

    let batch2 = state_after
        .batch("batch-000002")
        .expect("batch-000002 should exist");
    assert_eq!(batch2.phase, BatchPhase::CopyCompleted);
    assert!(!batch2.verification_passed);

    assert_eq!(
        fs::read_to_string(dest_dir.join("file2.txt")).expect("read copied file2"),
        "bbbb"
    );
}

#[test]
fn transfer_rerun_keeps_verified_batch_and_finishes_deletion_when_interactive() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    fs::write(source_dir.join("file1.txt"), "same").expect("write source file");
    fs::write(dest_dir.join("file1.txt"), "same").expect("write destination file");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 4;
    state.migration_phase = MigrationPhase::AwaitingDeletion;
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::VerifyCompleted,
        verification_passed: true,
        approved_for_delete: false,
        deleted: false,
    });

    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    persist_state(&state_path, &state).expect("persist state");

    let binary = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(binary)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "4B",
            "--interactive",
        ])
        .write_stdin("y\n")
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(
        output.status.success(),
        "interactive rerun should complete approved deletion path"
    );
    assert!(
        !source_dir.join("file1.txt").exists(),
        "source should be deleted after interactive approval"
    );

    let state_after = load_state(&state_path).expect("load state after interactive rerun");
    let batch = state_after
        .batch("batch-000001")
        .expect("batch-000001 should exist");
    assert_eq!(batch.phase, BatchPhase::DeleteCompleted);
    assert!(batch.verification_passed);
    assert!(batch.approved_for_delete);
    assert!(batch.deleted);
}
