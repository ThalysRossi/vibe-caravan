use caravan::error::CaravanError;
use caravan::models::state::{
    BatchPhase, BatchState, JournalEntry, MigrationState, PlannedBatch, PlannedFile,
};
use caravan::state_store::{load_state, persist_state};
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn migration_state_new_sets_expected_defaults() {
    let state = MigrationState::new("staging", "/src", "/dst");
    assert_eq!(state.mode, "staging");
    assert_eq!(state.source, "/src");
    assert_eq!(state.destination, "/dst");
    assert_eq!(state.max_files, None);
    assert_eq!(state.snapshot_every, None);
    assert_eq!(state.copy_buffer_size, 16 * 1024 * 1024);
    assert_eq!(state.buffered_copy_threshold, 8 * 1024 * 1024);
    assert!(state.planned_batches.is_empty());
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
fn approve_batches_handles_states_pushed_directly_into_vector() {
    let mut state = MigrationState::new("staging", "/src", "/dst");
    state.batches.push(BatchState {
        batch_id: "batch-1".to_string(),
        phase: BatchPhase::VerifyCompleted,
        verification_passed: true,
        approved_for_delete: false,
        deleted: false,
    });
    state.batches.push(BatchState {
        batch_id: "batch-2".to_string(),
        phase: BatchPhase::VerifyCompleted,
        verification_passed: true,
        approved_for_delete: false,
        deleted: false,
    });

    state.approve_batches(&["batch-2".to_string()]);

    let batch_1 = state.batch("batch-1").expect("batch-1 should exist");
    assert!(!batch_1.approved_for_delete);
    let batch_2 = state.batch("batch-2").expect("batch-2 should exist");
    assert!(batch_2.approved_for_delete);
    assert_eq!(batch_2.phase, BatchPhase::ApprovedForDelete);
}

#[test]
fn planned_batch_lookup_handles_batches_pushed_directly_into_vector() {
    let mut state = MigrationState::new("staging", "/src", "/dst");
    state.planned_batches.push(PlannedBatch {
        batch_id: "batch-1".to_string(),
        file_count: 1,
        total_bytes: 123,
        files: vec![PlannedFile {
            relative_path: PathBuf::from("dir/file.txt"),
            size_bytes: 123,
        }],
    });

    let planned = state
        .planned_batch("batch-1")
        .expect("planned batch should exist");
    assert_eq!(planned.batch_id, "batch-1");
    assert_eq!(planned.file_count, 1);
}

#[test]
fn persist_and_load_state_round_trip() {
    let tmp = TempDir::new().expect("temp dir");
    let state_path = tmp.path().join("state").join("state.json");

    let mut state = MigrationState::new("migrate", "/source", "/dest");
    state.max_files = Some(128);
    state.snapshot_every = Some(3);
    state.copy_buffer_size = 4 * 1024 * 1024;
    state.buffered_copy_threshold = 2 * 1024 * 1024;
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
fn load_legacy_state_defaults_new_resume_fields() {
    let tmp = TempDir::new().expect("temp dir");
    let state_path = tmp.path().join("legacy.json");
    let legacy_json = r#"{
  "mode": "staging",
  "source": "/source",
  "destination": "/dest",
  "batch_size_bytes": 1048576,
  "migration_phase": "Copying",
  "last_successful_snapshot_name": null,
  "batches": [],
  "journal": []
}"#;

    std::fs::write(&state_path, legacy_json).expect("write legacy state");

    let loaded = load_state(&state_path).expect("legacy state should load");
    assert_eq!(loaded.max_files, None);
    assert_eq!(loaded.snapshot_every, None);
    assert_eq!(loaded.copy_buffer_size, 16 * 1024 * 1024);
    assert_eq!(loaded.buffered_copy_threshold, 8 * 1024 * 1024);
    assert!(loaded.planned_batches.is_empty());
}

#[test]
fn load_state_errors_for_missing_file() {
    let tmp = TempDir::new().expect("temp dir");
    let missing = tmp.path().join("nope.json");
    let err = load_state(&missing).expect_err("load should fail");
    match err {
        CaravanError::StateRead { .. } => {}
        _ => panic!("expected CaravanError::StateRead"),
    }
}

#[test]
fn persist_state_creates_parent_directories() {
    let tmp = TempDir::new().expect("temp dir");
    let state_path = tmp.path().join("deep").join("dir").join("state.json");

    let state = MigrationState::new("staging", "/source", "/dest");
    persist_state(&state_path, &state).expect("state should persist");

    assert!(state_path.exists(), "state file should exist");
    let loaded = load_state(&state_path).expect("should load state");
    assert_eq!(loaded, state);
}

#[cfg(target_os = "linux")]
#[test]
fn persist_state_replaces_file_atomically_for_existing_readers() {
    use std::io::{Read, Seek, SeekFrom};

    let tmp = TempDir::new().expect("temp dir");
    let state_path = tmp.path().join("state.json");

    let mut old_state = MigrationState::new("staging", "/source-a", "/dest-a");
    old_state.batch_size_bytes = 1024;
    persist_state(&state_path, &old_state).expect("persist initial state");

    let mut open_reader = std::fs::File::open(&state_path).expect("open existing state");

    let mut new_state = MigrationState::new("migrate", "/source-b", "/dest-b");
    new_state.batch_size_bytes = 2048;
    persist_state(&state_path, &new_state).expect("persist replacement state");

    open_reader
        .seek(SeekFrom::Start(0))
        .expect("rewind old reader");
    let mut old_view = String::new();
    open_reader
        .read_to_string(&mut old_view)
        .expect("read old fd");

    let latest_view = std::fs::read_to_string(&state_path).expect("read new path");

    assert!(
        old_view.contains("\"source\": \"/source-a\""),
        "existing file descriptor should still see the pre-replacement contents"
    );
    assert!(
        latest_view.contains("\"source\": \"/source-b\""),
        "path should resolve to the replacement contents"
    );
}
