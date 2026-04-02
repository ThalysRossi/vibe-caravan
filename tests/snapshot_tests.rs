use std::path::Path;

use wololo::config::Mode;
use wololo::error::WololoError;
use wololo::models::state::{BatchPhase, BatchState, MigrationState};
use wololo::snapshot::{snapshot_if_needed, SnapshotBackend};

struct StubSnapshotBackend {
    snapshot_name: Option<String>,
    fail_message: Option<String>,
}

impl SnapshotBackend for StubSnapshotBackend {
    fn create_snapshot(
        &self,
        _destination_root: &Path,
        _batch_id: &str,
    ) -> Result<String, WololoError> {
        if let Some(message) = &self.fail_message {
            return Err(WololoError::InvalidArguments(message.clone()));
        }
        self.snapshot_name
            .clone()
            .ok_or_else(|| WololoError::InvalidArguments("missing snapshot name".to_string()))
    }
}

#[test]
fn snapshots_are_rejected_in_staging_mode() {
    let mut state = MigrationState::new("staging", "/src", "/dst");
    let backend = StubSnapshotBackend {
        snapshot_name: Some("snap-1".to_string()),
        fail_message: None,
    };

    let err = snapshot_if_needed(
        Mode::Staging,
        Some(1),
        1,
        "batch-1",
        Path::new("/dst"),
        &mut state,
        &backend,
    )
    .expect_err("staging snapshots should be rejected");
    assert!(err
        .to_string()
        .contains("snapshots are only supported in migrate mode"));
}

#[test]
fn snapshot_creation_is_invoked_only_in_migrate_mode_and_cadence() {
    let mut state = MigrationState::new("migrate", "/src", "/dst");
    state.upsert_batch(BatchState {
        batch_id: "batch-2".to_string(),
        phase: BatchPhase::DeleteCompleted,
        verification_passed: true,
        approved_for_delete: true,
        deleted: true,
    });

    let backend = StubSnapshotBackend {
        snapshot_name: Some("snap-2".to_string()),
        fail_message: None,
    };

    let skipped = snapshot_if_needed(
        Mode::Migrate,
        Some(2),
        1,
        "batch-2",
        Path::new("/dst"),
        &mut state,
        &backend,
    )
    .expect("non-cadence run should not fail");
    assert_eq!(skipped, None);

    let created = snapshot_if_needed(
        Mode::Migrate,
        Some(2),
        2,
        "batch-2",
        Path::new("/dst"),
        &mut state,
        &backend,
    )
    .expect("cadence-aligned run should succeed");
    assert_eq!(created, Some("snap-2".to_string()));
}

#[test]
fn snapshot_failures_are_recorded() {
    let mut state = MigrationState::new("migrate", "/src", "/dst");
    let backend = StubSnapshotBackend {
        snapshot_name: None,
        fail_message: Some("backend failed".to_string()),
    };

    let err = snapshot_if_needed(
        Mode::Migrate,
        Some(1),
        1,
        "batch-3",
        Path::new("/dst"),
        &mut state,
        &backend,
    )
    .expect_err("snapshot failure should bubble up");
    assert!(err.to_string().contains("backend failed"));
    assert_eq!(state.journal.len(), 1);
    assert_eq!(state.journal[0].event, "snapshot_failed");
}

#[test]
fn snapshot_metadata_is_persisted_in_state() {
    let mut state = MigrationState::new("migrate", "/src", "/dst");
    state.upsert_batch(BatchState {
        batch_id: "batch-4".to_string(),
        phase: BatchPhase::DeleteCompleted,
        verification_passed: true,
        approved_for_delete: true,
        deleted: true,
    });

    let backend = StubSnapshotBackend {
        snapshot_name: Some("snap-4".to_string()),
        fail_message: None,
    };
    snapshot_if_needed(
        Mode::Migrate,
        Some(1),
        1,
        "batch-4",
        Path::new("/dst"),
        &mut state,
        &backend,
    )
    .expect("snapshot should succeed");

    assert_eq!(
        state.last_successful_snapshot_name,
        Some("snap-4".to_string())
    );
    assert_eq!(state.journal.len(), 1);
    assert_eq!(state.journal[0].event, "snapshot_completed");
    let batch = state.batch("batch-4").expect("batch should exist");
    assert_eq!(batch.phase, BatchPhase::SnapshotCompleted);
}
