use std::fs;

use assert_cmd::Command;
use caravan::migration_registry;
use caravan::models::state::{BatchPhase, BatchState, MigrationPhase, MigrationState};
use caravan::state_store::{load_state, persist_state};
use tempfile::TempDir;

#[test]
fn user_scenario_rerun_transfer_skips_already_copied_batches() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    for i in 1..=7 {
        fs::write(source_dir.join(format!("file{i}.txt")), format!("f{i:03}"))
            .expect("write source file");
    }

    for i in 1..=3 {
        fs::write(dest_dir.join(format!("file{i}.txt")), format!("f{i:03}"))
            .expect("write destination file");
    }
    fs::write(dest_dir.join("file1.txt"), "xxxx").expect("write mismatched destination file1");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 4;
    state.migration_phase = MigrationPhase::Copying;

    for i in 1..=3 {
        state.upsert_batch(BatchState {
            batch_id: format!("batch-{i:06}"),
            phase: BatchPhase::CopyCompleted,
            verification_passed: false,
            approved_for_delete: false,
            deleted: false,
        });
    }
    for i in 4..=7 {
        state.upsert_batch(BatchState {
            batch_id: format!("batch-{i:06}"),
            phase: BatchPhase::Planned,
            verification_passed: false,
            approved_for_delete: false,
            deleted: false,
        });
    }

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
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(
        !output.status.success(),
        "rerun should fail verification for mismatched pre-copied batch"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("verification failed"),
        "expected verification failure, got: {stderr}"
    );

    let state_after = load_state(&state_path).expect("load state after rerun");
    assert_eq!(state_after.batches.len(), 7);

    let batch1 = state_after
        .batch("batch-000001")
        .expect("batch-000001 should exist");
    assert_eq!(batch1.phase, BatchPhase::VerifyCompleted);
    assert!(!batch1.verification_passed);

    let batch4 = state_after
        .batch("batch-000004")
        .expect("batch-000004 should exist");
    assert_eq!(batch4.phase, BatchPhase::CopyCompleted);
    assert!(!batch4.verification_passed);
    assert!(dest_dir.join("file4.txt").exists());
}

#[test]
fn state_initialization_does_not_overwrite_progress() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    fs::write(source_dir.join("file1.txt"), "aaaa").expect("write source file1");
    fs::write(source_dir.join("file2.txt"), "bbbb").expect("write source file2");
    fs::write(dest_dir.join("file1.txt"), "zzzz").expect("write mismatched destination file1");

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
        phase: BatchPhase::Planned,
        verification_passed: false,
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
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(
        !output.status.success(),
        "rerun should fail verification instead of recopying mismatched CopyCompleted batch"
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
    assert_eq!(
        fs::read_to_string(dest_dir.join("file2.txt")).expect("read copied file2"),
        "bbbb"
    );
}
