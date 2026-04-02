use tempfile::TempDir;
use wololo::models::state::{BatchPhase, BatchState, JournalEntry, MigrationState};
use wololo::state_store::{load_state, persist_state};

#[test]
fn migration_state_new_sets_expected_defaults() {
    let state = MigrationState::new("staging", "/src", "/dst");
    assert_eq!(state.mode, "staging");
    assert_eq!(state.source, "/src");
    assert_eq!(state.destination, "/dst");
    assert_eq!(state.last_successful_snapshot_name, None);
    assert!(state.batches.is_empty());
    assert!(state.journal.is_empty());
}

#[test]
fn upsert_batch_inserts_and_updates_by_batch_id() {
    let mut state = MigrationState::new("staging", "/src", "/dst");
    state.upsert_batch(BatchState {
        batch_id: "batch-1".to_string(),
        phase: BatchPhase::Planned,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });
    state.upsert_batch(BatchState {
        batch_id: "batch-1".to_string(),
        phase: BatchPhase::ApprovedForDelete,
        verification_passed: true,
        approved_for_delete: true,
        deleted: false,
    });

    assert_eq!(state.batches.len(), 1);
    let batch = state.batch("batch-1").expect("batch should exist");
    assert_eq!(batch.phase, BatchPhase::ApprovedForDelete);
    assert!(batch.verification_passed);
}

#[test]
fn persist_and_load_state_round_trip() {
    let tmp = TempDir::new().expect("temp dir");
    let state_path = tmp.path().join("state").join("state.json");

    let mut state = MigrationState::new("migrate", "/source", "/dest");
    state.upsert_batch(BatchState {
        batch_id: "batch-123".to_string(),
        phase: BatchPhase::VerifyCompleted,
        verification_passed: true,
        approved_for_delete: false,
        deleted: false,
    });
    state.journal.push(JournalEntry {
        event: "verify_completed".to_string(),
        batch_id: "batch-123".to_string(),
        timestamp_unix_secs: 100,
        context: "test".to_string(),
    });

    persist_state(&state_path, &state).expect("state should persist");
    let loaded = load_state(&state_path).expect("state should load");
    assert_eq!(loaded, state);
}

#[test]
fn load_state_errors_for_missing_file() {
    let tmp = TempDir::new().expect("temp dir");
    let missing = tmp.path().join("nope.json");
    let err = load_state(&missing).expect_err("load should fail");
    assert!(err.to_string().contains("failed to read state file"));
}
