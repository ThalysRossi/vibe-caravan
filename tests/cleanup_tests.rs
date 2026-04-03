use std::fs;

use tempfile::TempDir;
use caravan::cleanup::cleanup_batch;
use caravan::models::state::{BatchPhase, BatchState, MigrationState};
use caravan::plan::{build_plan, PlanOptions};

fn create_file(root: &std::path::Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent dirs should be created");
    }
    fs::write(path, bytes).expect("file should be created");
}

#[test]
fn cleanup_is_blocked_when_verification_failed() {
    let src = TempDir::new().expect("source temp dir");
    create_file(src.path(), "x/a.txt", b"a");

    let plan = build_plan(
        src.path(),
        &PlanOptions {
            batch_size_bytes: 1024,
            max_files: Some(10),
        },
    )
    .expect("planning should succeed");
    let batch = plan.batches[0].clone();

    let mut state = MigrationState::new("staging", "/src", "/dst");
    state.upsert_batch(BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::VerifyCompleted,
        verification_passed: false,
        approved_for_delete: true,
        deleted: false,
    });

    let err =
        cleanup_batch(&batch, src.path(), &mut state, "test").expect_err("cleanup should fail");
    assert!(err
        .to_string()
        .contains("deletion blocked because verification did not pass"));
}

#[test]
fn cleanup_is_blocked_without_approval() {
    let src = TempDir::new().expect("source temp dir");
    create_file(src.path(), "x/a.txt", b"a");

    let plan = build_plan(
        src.path(),
        &PlanOptions {
            batch_size_bytes: 1024,
            max_files: Some(10),
        },
    )
    .expect("planning should succeed");
    let batch = plan.batches[0].clone();

    let mut state = MigrationState::new("staging", "/src", "/dst");
    state.upsert_batch(BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::VerifyCompleted,
        verification_passed: true,
        approved_for_delete: false,
        deleted: false,
    });

    let err =
        cleanup_batch(&batch, src.path(), &mut state, "test").expect_err("cleanup should fail");
    assert!(err
        .to_string()
        .contains("deletion blocked because batch is not approved"));
}

#[test]
fn cleanup_deletes_files_and_journals_result() {
    let src = TempDir::new().expect("source temp dir");
    create_file(src.path(), "x/a.txt", b"a");
    create_file(src.path(), "x/b.txt", b"b");

    let plan = build_plan(
        src.path(),
        &PlanOptions {
            batch_size_bytes: 1024,
            max_files: Some(10),
        },
    )
    .expect("planning should succeed");
    let batch = plan.batches[0].clone();

    let mut state = MigrationState::new("staging", "/src", "/dst");
    state.upsert_batch(BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::ApprovedForDelete,
        verification_passed: true,
        approved_for_delete: true,
        deleted: false,
    });

    cleanup_batch(&batch, src.path(), &mut state, "test-context")
        .expect("cleanup should succeed");

    assert!(!src.path().join("x/a.txt").exists());
    assert!(!src.path().join("x/b.txt").exists());

    let updated = state.batch(&batch.id).expect("updated batch state should exist");
    assert!(updated.deleted);
    assert_eq!(updated.phase, BatchPhase::DeleteCompleted);
    assert_eq!(state.journal.len(), 1);
    assert_eq!(state.journal[0].event, "delete_completed");
}

#[test]
fn cleanup_is_idempotent_when_batch_already_deleted() {
    let src = TempDir::new().expect("source temp dir");
    create_file(src.path(), "x/a.txt", b"a");

    let plan = build_plan(
        src.path(),
        &PlanOptions {
            batch_size_bytes: 1024,
            max_files: Some(10),
        },
    )
    .expect("planning should succeed");
    let batch = plan.batches[0].clone();

    let mut state = MigrationState::new("staging", "/src", "/dst");
    state.upsert_batch(BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::DeleteCompleted,
        verification_passed: true,
        approved_for_delete: true,
        deleted: true,
    });

    cleanup_batch(&batch, src.path(), &mut state, "test-context")
        .expect("already-deleted batch should be no-op");
    assert_eq!(state.journal.len(), 0);
}
