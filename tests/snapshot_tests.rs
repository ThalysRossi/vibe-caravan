use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use caravan::config::Mode;
use caravan::error::CaravanError;
use caravan::models::state::{BatchPhase, BatchState, MigrationState};
use caravan::snapshot::{
    process_pending_snapshots, snapshot_if_needed, validate_snapshot_configuration,
    SnapshotBackend, SnapshotRequest,
};
use tempfile::TempDir;

struct StubSnapshotBackend {
    snapshot_name: Option<String>,
    fail_message: Option<String>,
}

impl SnapshotBackend for StubSnapshotBackend {
    fn create_snapshot(
        &self,
        _destination_root: &Path,
        _snapshot_root: Option<&Path>,
        _batch_id: &str,
    ) -> Result<String, CaravanError> {
        if let Some(message) = &self.fail_message {
            return Err(CaravanError::InvalidArguments(message.clone()));
        }
        self.snapshot_name
            .clone()
            .ok_or_else(|| CaravanError::InvalidArguments("missing snapshot name".to_string()))
    }
}

struct ScriptedSnapshotBackend {
    failing_batches: HashSet<String>,
    calls: RefCell<Vec<String>>,
}

impl ScriptedSnapshotBackend {
    fn with_failing_batches(failing_batches: &[&str]) -> Self {
        Self {
            failing_batches: failing_batches
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl SnapshotBackend for ScriptedSnapshotBackend {
    fn create_snapshot(
        &self,
        _destination_root: &Path,
        _snapshot_root: Option<&Path>,
        batch_id: &str,
    ) -> Result<String, CaravanError> {
        self.calls.borrow_mut().push(batch_id.to_string());
        if self.failing_batches.contains(batch_id) {
            return Err(CaravanError::InvalidArguments(format!(
                "snapshot backend failure for {batch_id}"
            )));
        }
        Ok(format!("snap-{batch_id}"))
    }
}

struct SnapshotRootAssertingBackend {
    expected_snapshot_root: PathBuf,
}

impl SnapshotBackend for SnapshotRootAssertingBackend {
    fn create_snapshot(
        &self,
        _destination_root: &Path,
        snapshot_root: Option<&Path>,
        _batch_id: &str,
    ) -> Result<String, CaravanError> {
        let snapshot_root = snapshot_root.ok_or_else(|| {
            CaravanError::InvalidArguments("snapshot root should be provided".to_string())
        })?;
        if snapshot_root != self.expected_snapshot_root.as_path() {
            return Err(CaravanError::InvalidArguments(format!(
                "unexpected snapshot root '{}'",
                snapshot_root.display()
            )));
        }
        Ok("snap-with-custom-root".to_string())
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
        SnapshotRequest {
            mode: Mode::Staging,
            snapshot_every: Some(1),
            completed_batch_count: 1,
            batch_id: "batch-1",
            destination_root: Path::new("/dst"),
            snapshot_root: None,
        },
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
        SnapshotRequest {
            mode: Mode::Migrate,
            snapshot_every: Some(2),
            completed_batch_count: 1,
            batch_id: "batch-2",
            destination_root: Path::new("/dst"),
            snapshot_root: None,
        },
        &mut state,
        &backend,
    )
    .expect("non-cadence run should not fail");
    assert_eq!(skipped, None);

    let created = snapshot_if_needed(
        SnapshotRequest {
            mode: Mode::Migrate,
            snapshot_every: Some(2),
            completed_batch_count: 2,
            batch_id: "batch-2",
            destination_root: Path::new("/dst"),
            snapshot_root: None,
        },
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
        SnapshotRequest {
            mode: Mode::Migrate,
            snapshot_every: Some(1),
            completed_batch_count: 1,
            batch_id: "batch-3",
            destination_root: Path::new("/dst"),
            snapshot_root: None,
        },
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
        SnapshotRequest {
            mode: Mode::Migrate,
            snapshot_every: Some(1),
            completed_batch_count: 1,
            batch_id: "batch-4",
            destination_root: Path::new("/dst"),
            snapshot_root: None,
        },
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

#[test]
fn process_pending_snapshots_applies_cadence_to_deleted_batches() {
    let mut state = MigrationState::new("migrate", "/src", "/dst");
    for batch_id in [
        "batch-000001",
        "batch-000002",
        "batch-000003",
        "batch-000004",
    ] {
        state.upsert_batch(BatchState {
            batch_id: batch_id.to_string(),
            phase: BatchPhase::DeleteCompleted,
            verification_passed: true,
            approved_for_delete: true,
            deleted: true,
        });
    }

    let backend = ScriptedSnapshotBackend::with_failing_batches(&[]);
    let persist_count = Cell::new(0u32);
    let mut persist_state = |_current_state: &MigrationState| {
        persist_count.set(persist_count.get().saturating_add(1));
        Ok::<(), CaravanError>(())
    };

    process_pending_snapshots(
        Mode::Migrate,
        Some(2),
        Path::new("/dst"),
        None,
        &mut state,
        &backend,
        &mut persist_state,
    )
    .expect("snapshot phase should succeed");

    assert_eq!(
        backend.calls(),
        vec!["batch-000002".to_string(), "batch-000004".to_string()]
    );
    assert_eq!(persist_count.get(), 2);
    assert_eq!(
        state.batch("batch-000002").unwrap().phase,
        BatchPhase::SnapshotCompleted
    );
    assert_eq!(
        state.batch("batch-000004").unwrap().phase,
        BatchPhase::SnapshotCompleted
    );
}

#[test]
fn process_pending_snapshots_continues_after_snapshot_failure() {
    let mut state = MigrationState::new("migrate", "/src", "/dst");
    for batch_id in ["batch-000001", "batch-000002"] {
        state.upsert_batch(BatchState {
            batch_id: batch_id.to_string(),
            phase: BatchPhase::DeleteCompleted,
            verification_passed: true,
            approved_for_delete: true,
            deleted: true,
        });
    }

    let backend = ScriptedSnapshotBackend::with_failing_batches(&["batch-000001"]);
    let persist_count = Cell::new(0u32);
    let mut persist_state = |_current_state: &MigrationState| {
        persist_count.set(persist_count.get().saturating_add(1));
        Ok::<(), CaravanError>(())
    };

    process_pending_snapshots(
        Mode::Migrate,
        Some(1),
        Path::new("/dst"),
        None,
        &mut state,
        &backend,
        &mut persist_state,
    )
    .expect("snapshot failures should be non-fatal");

    assert_eq!(
        backend.calls(),
        vec!["batch-000001".to_string(), "batch-000002".to_string()]
    );
    assert_eq!(persist_count.get(), 2);
    assert!(state
        .journal
        .iter()
        .any(|entry| entry.event == "snapshot_failed"));
    assert!(state
        .journal
        .iter()
        .any(|entry| entry.event == "snapshot_completed"));
}

#[test]
fn process_pending_snapshots_skips_batches_already_snapshot_completed() {
    let mut state = MigrationState::new("migrate", "/src", "/dst");
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::SnapshotCompleted,
        verification_passed: true,
        approved_for_delete: true,
        deleted: true,
    });
    state.upsert_batch(BatchState {
        batch_id: "batch-000002".to_string(),
        phase: BatchPhase::DeleteCompleted,
        verification_passed: true,
        approved_for_delete: true,
        deleted: true,
    });

    let backend = ScriptedSnapshotBackend::with_failing_batches(&[]);
    let persist_count = Cell::new(0u32);
    let mut persist_state = |_current_state: &MigrationState| {
        persist_count.set(persist_count.get().saturating_add(1));
        Ok::<(), CaravanError>(())
    };

    process_pending_snapshots(
        Mode::Migrate,
        Some(1),
        Path::new("/dst"),
        None,
        &mut state,
        &backend,
        &mut persist_state,
    )
    .expect("snapshot phase should succeed");

    assert_eq!(backend.calls(), vec!["batch-000002".to_string()]);
    assert_eq!(persist_count.get(), 1);
}

#[test]
fn process_pending_snapshots_forwards_custom_snapshot_root() {
    let temp = TempDir::new().expect("temp directory should be created");
    let destination_root = temp.path().join("dest");
    let snapshot_root = temp.path().join("snapshots");
    std::fs::create_dir_all(&destination_root).expect("destination root should be created");
    std::fs::create_dir_all(&snapshot_root).expect("snapshot root should be created");

    let mut state = MigrationState::new("migrate", "/src", "/dst");
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::DeleteCompleted,
        verification_passed: true,
        approved_for_delete: true,
        deleted: true,
    });

    let backend = SnapshotRootAssertingBackend {
        expected_snapshot_root: snapshot_root.clone(),
    };
    let mut persist_state = |_current_state: &MigrationState| Ok::<(), CaravanError>(());

    process_pending_snapshots(
        Mode::Migrate,
        Some(1),
        &destination_root,
        Some(&snapshot_root),
        &mut state,
        &backend,
        &mut persist_state,
    )
    .expect("snapshot run should use custom snapshot root");

    assert_eq!(
        state.last_successful_snapshot_name,
        Some("snap-with-custom-root".to_string())
    );
}

#[test]
fn validate_snapshot_configuration_rejects_snapshot_root_without_cadence() {
    let err = validate_snapshot_configuration(
        Mode::Migrate,
        None,
        Path::new("/dst"),
        Some(Path::new("/dst/snapshots")),
    )
    .expect_err("snapshot root without cadence should fail");

    assert!(err
        .to_string()
        .contains("snapshot-dir requires snapshot-every"));
}

#[test]
fn validate_snapshot_configuration_rejects_non_directory_snapshot_root() {
    let temp = TempDir::new().expect("temp directory should be created");
    let destination_root = temp.path().join("dest");
    let snapshot_file = temp.path().join("snapshot-file");
    std::fs::create_dir_all(&destination_root).expect("destination root should be created");
    std::fs::write(&snapshot_file, b"not-a-directory")
        .expect("snapshot path fixture file should be created");

    let err = validate_snapshot_configuration(
        Mode::Migrate,
        Some(1),
        &destination_root,
        Some(&snapshot_file),
    )
    .expect_err("non-directory snapshot root should fail");

    assert!(err
        .to_string()
        .contains("snapshot destination must be an existing directory"));
}

#[test]
fn validate_snapshot_configuration_rejects_snapshot_root_inside_destination() {
    let temp = TempDir::new().expect("temp directory should be created");
    let destination_root = temp.path().join("dest");
    let nested_snapshot_root = destination_root.join("snapshots");
    std::fs::create_dir_all(&nested_snapshot_root).expect("nested snapshot root should be created");

    let err = validate_snapshot_configuration(
        Mode::Migrate,
        Some(1),
        &destination_root,
        Some(&nested_snapshot_root),
    )
    .expect_err("snapshot root inside destination should fail");

    assert!(err
        .to_string()
        .contains("snapshot destination must not be inside migration destination"));
}

#[test]
fn validate_snapshot_configuration_allows_snapshot_root_with_parent_segments_outside_destination() {
    let temp = TempDir::new().expect("temp directory should be created");
    let destination_root = temp.path().join("dest");
    let snapshot_root = temp.path().join("snapshots");
    std::fs::create_dir_all(&destination_root).expect("destination root should be created");
    std::fs::create_dir_all(&snapshot_root).expect("snapshot root should be created");

    let snapshot_with_parent_segments = destination_root.join("../snapshots");
    validate_snapshot_configuration(
        Mode::Migrate,
        Some(1),
        &destination_root,
        Some(&snapshot_with_parent_segments),
    )
    .expect("normalized snapshot root outside destination should be accepted");
}
