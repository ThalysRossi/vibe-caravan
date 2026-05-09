use std::fs;

use caravan::models::state::{BatchPhase, MigrationPhase};
use caravan::state_store::{load_state, persist_state};

mod common;

use common::{batch_state, migration_state_for_fixture, run_resume, source_dest_fixture};

#[test]
fn resume_with_copy_completed_batches_should_verify_not_copy() {
    let fixture = source_dest_fixture();
    let source_dir = &fixture.source_dir;
    let dest_dir = &fixture.dest_dir;

    fs::write(source_dir.join("file1.txt"), "aaaa").expect("create source file1");
    fs::write(source_dir.join("file2.txt"), "bbbb").expect("create source file2");
    fs::write(dest_dir.join("file1.txt"), "aaaa").expect("create dest file1");
    fs::write(dest_dir.join("file2.txt"), "bbbb").expect("create dest file2");

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
        BatchPhase::CopyCompleted,
        false,
        false,
        false,
    ));

    let state_path = fixture.tmp.path().join("resume-state.json");
    persist_state(&state_path, &state).expect("persist state");

    let output = run_resume(&state_path, fixture.tmp.path());

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
    let fixture = source_dest_fixture();
    let source_dir = &fixture.source_dir;
    let dest_dir = &fixture.dest_dir;

    fs::write(source_dir.join("file1.txt"), "aaaa").expect("create source file");

    let mut state =
        migration_state_for_fixture("staging", source_dir, dest_dir, 4, MigrationPhase::Copying);
    state.upsert_batch(batch_state(
        "batch-000001",
        BatchPhase::CopyCompleted,
        false,
        false,
        false,
    ));

    let state_path = fixture.tmp.path().join("resume-state.json");
    persist_state(&state_path, &state).expect("persist state");

    let output = run_resume(&state_path, fixture.tmp.path());

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
