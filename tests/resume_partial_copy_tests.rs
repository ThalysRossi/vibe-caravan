use std::fs;

use caravan::models::state::{BatchPhase, MigrationPhase};
use caravan::state_store::{load_state, persist_state};

mod common;

use common::{batch_state, migration_state_for_fixture, run_resume, source_dest_fixture};

#[test]
fn resume_skips_already_copied_batches_and_verifies() {
    let fixture = source_dest_fixture();
    let source_dir = &fixture.source_dir;
    let dest_dir = &fixture.dest_dir;

    fs::write(source_dir.join("file1.txt"), "aaaa").expect("create source file1");
    fs::write(source_dir.join("file2.txt"), "bbbb").expect("create source file2");
    fs::write(source_dir.join("file3.txt"), "cccc").expect("create source file3");

    fs::write(dest_dir.join("file1.txt"), "aaaa").expect("create destination file1");
    fs::write(dest_dir.join("file2.txt"), "bbbb").expect("create destination file2");

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
    state.upsert_batch(batch_state(
        "batch-000003",
        BatchPhase::Planned,
        false,
        false,
        false,
    ));

    let state_path = fixture.tmp.path().join("resume-state.json");
    persist_state(&state_path, &state).expect("persist state");

    let output = run_resume(&state_path, fixture.tmp.path());

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
