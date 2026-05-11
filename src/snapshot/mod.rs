use std::time::{SystemTime, UNIX_EPOCH};

mod backend;
mod paths;
mod policy;
mod processing;
mod request;
mod validation;

#[cfg(target_os = "linux")]
mod platform_linux;
#[cfg(target_os = "windows")]
mod platform_windows;

pub use backend::{SnapshotBackend, SystemSnapshotBackend};
pub use policy::snapshot_if_needed;
pub use processing::{
    SnapshotProgressReporter, process_pending_snapshots, process_pending_snapshots_with_interrupt,
    process_pending_snapshots_with_progress, process_pending_snapshots_with_progress_and_interrupt,
};
pub use request::SnapshotRequest;
pub use validation::validate_snapshot_configuration;

pub(crate) fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    use tempfile::tempdir;

    use crate::config::Mode;
    use crate::error::CaravanError;
    use crate::models::state::{BatchPhase, BatchState, MigrationPhase, MigrationState};
    use crate::snapshot::paths::{
        canonical_path, canonical_path_for_maybe_missing, path_within_or_equal,
        resolve_existing_path,
    };
    #[cfg(target_os = "linux")]
    use crate::snapshot::platform_linux::create_btrfs_snapshot;

    #[derive(Default)]
    struct RecordingSnapshotBackend {
        failed_batches: HashSet<String>,
        calls: RefCell<Vec<String>>,
    }

    impl RecordingSnapshotBackend {
        fn fail_for(mut self, batch_id: &str) -> Self {
            self.failed_batches.insert(batch_id.to_string());
            self
        }
    }

    impl SnapshotBackend for RecordingSnapshotBackend {
        fn create_snapshot(
            &self,
            _destination_root: &Path,
            _snapshot_root: Option<&Path>,
            batch_id: &str,
        ) -> Result<String, CaravanError> {
            self.calls.borrow_mut().push(batch_id.to_string());
            if self.failed_batches.contains(batch_id) {
                return Err(CaravanError::InvalidArguments(format!(
                    "snapshot failed for {batch_id}"
                )));
            }
            Ok(format!("snap-{batch_id}"))
        }
    }

    fn build_state(batches: &[(&str, BatchPhase, bool)]) -> MigrationState {
        let mut state = MigrationState::new("migrate", "/src", "/dst");
        state.migration_phase = MigrationPhase::Copying;
        state.batches = batches
            .iter()
            .map(|(id, phase, deleted)| BatchState {
                batch_id: (*id).to_string(),
                phase: *phase,
                verification_passed: true,
                approved_for_delete: *deleted,
                deleted: *deleted,
            })
            .collect();
        state.rebuild_indexes();
        state
    }

    #[test]
    fn snapshot_if_needed_rejects_snapshots_in_staging_mode() {
        let backend = RecordingSnapshotBackend::default();
        let mut state = build_state(&[]);
        let temp = tempdir().expect("tempdir");

        let err = snapshot_if_needed(
            SnapshotRequest {
                mode: Mode::Staging,
                snapshot_every: Some(1),
                completed_batch_count: 1,
                batch_id: "batch-1",
                destination_root: temp.path(),
                snapshot_root: None,
            },
            &mut state,
            &backend,
        )
        .expect_err("staging snapshot should fail");

        assert!(err.to_string().contains("only supported in migrate mode"));
    }

    #[test]
    fn snapshot_if_needed_skips_when_cadence_not_reached() {
        let backend = RecordingSnapshotBackend::default();
        let mut state = build_state(&[]);
        let temp = tempdir().expect("tempdir");

        let result = snapshot_if_needed(
            SnapshotRequest {
                mode: Mode::Migrate,
                snapshot_every: Some(3),
                completed_batch_count: 2,
                batch_id: "batch-1",
                destination_root: temp.path(),
                snapshot_root: None,
            },
            &mut state,
            &backend,
        )
        .expect("snapshot check should succeed");

        assert_eq!(result, None);
        assert!(backend.calls.borrow().is_empty());
    }

    #[test]
    fn snapshot_if_needed_rejects_zero_cadence() {
        let backend = RecordingSnapshotBackend::default();
        let mut state = build_state(&[]);
        let temp = tempdir().expect("tempdir");

        let err = snapshot_if_needed(
            SnapshotRequest {
                mode: Mode::Migrate,
                snapshot_every: Some(0),
                completed_batch_count: 1,
                batch_id: "batch-1",
                destination_root: temp.path(),
                snapshot_root: None,
            },
            &mut state,
            &backend,
        )
        .expect_err("zero cadence should fail");

        assert!(err.to_string().contains("must be greater than zero"));
    }

    #[test]
    fn snapshot_if_needed_updates_state_and_journal_on_success() {
        let backend = RecordingSnapshotBackend::default();
        let mut state = build_state(&[("batch-7", BatchPhase::DeleteCompleted, true)]);
        let temp = tempdir().expect("tempdir");

        let result = snapshot_if_needed(
            SnapshotRequest {
                mode: Mode::Migrate,
                snapshot_every: Some(2),
                completed_batch_count: 2,
                batch_id: "batch-7",
                destination_root: temp.path(),
                snapshot_root: None,
            },
            &mut state,
            &backend,
        )
        .expect("snapshot should succeed");

        assert_eq!(result.as_deref(), Some("snap-batch-7"));
        assert_eq!(
            state.last_successful_snapshot_name.as_deref(),
            Some("snap-batch-7")
        );
        assert_eq!(
            state.batch("batch-7").expect("batch exists").phase,
            BatchPhase::SnapshotCompleted
        );
        let journal = state.journal.last().expect("journal entry");
        assert_eq!(journal.event, "snapshot_completed");
        assert_eq!(journal.batch_id, "batch-7");
        assert_eq!(journal.context, "snap-batch-7");
        assert!(journal.timestamp_unix_secs > 1);
    }

    #[test]
    fn snapshot_if_needed_records_failure_in_journal() {
        let backend = RecordingSnapshotBackend::default().fail_for("batch-9");
        let mut state = build_state(&[("batch-9", BatchPhase::DeleteCompleted, true)]);
        let temp = tempdir().expect("tempdir");

        let err = snapshot_if_needed(
            SnapshotRequest {
                mode: Mode::Migrate,
                snapshot_every: Some(1),
                completed_batch_count: 1,
                batch_id: "batch-9",
                destination_root: temp.path(),
                snapshot_root: None,
            },
            &mut state,
            &backend,
        )
        .expect_err("snapshot should fail");

        assert!(err.to_string().contains("snapshot failed for batch-9"));
        assert_eq!(
            state.batch("batch-9").expect("batch exists").phase,
            BatchPhase::DeleteCompleted
        );
        let journal = state.journal.last().expect("journal entry");
        assert_eq!(journal.event, "snapshot_failed");
        assert_eq!(journal.batch_id, "batch-9");
        assert!(journal.context.contains("snapshot failed for batch-9"));
        assert!(journal.timestamp_unix_secs > 1);
    }

    #[test]
    fn validate_snapshot_configuration_rejects_snapshot_dir_without_cadence() {
        let temp = tempdir().expect("tempdir");
        let err =
            validate_snapshot_configuration(Mode::Migrate, None, temp.path(), Some(temp.path()))
                .expect_err("must fail");
        assert!(
            err.to_string()
                .contains("snapshot-dir requires snapshot-every")
        );
    }

    #[test]
    fn validate_snapshot_configuration_rejects_staging_snapshot_root_even_without_cadence() {
        let temp = tempdir().expect("tempdir");

        let err =
            validate_snapshot_configuration(Mode::Staging, None, temp.path(), Some(temp.path()))
                .expect_err("staging mode should reject snapshot options");

        assert!(
            err.to_string()
                .contains("snapshot-dir requires snapshot-every")
        );
    }

    #[test]
    fn validate_snapshot_configuration_rejects_staging_snapshot_cadence() {
        let temp = tempdir().expect("tempdir");

        let err = validate_snapshot_configuration(Mode::Staging, Some(1), temp.path(), None)
            .expect_err("staging mode should reject snapshot cadence");

        assert!(err.to_string().contains("only supported in migrate mode"));
    }

    #[test]
    fn validate_snapshot_configuration_rejects_snapshot_directory_within_destination() {
        let temp = tempdir().expect("tempdir");
        let destination = temp.path().join("dest");
        let snapshot = destination.join("snaps");
        std::fs::create_dir_all(&snapshot).expect("create snapshot dir");

        let err =
            validate_snapshot_configuration(Mode::Migrate, Some(1), &destination, Some(&snapshot))
                .expect_err("must reject nested snapshot dir");

        assert!(
            err.to_string()
                .contains("must not be inside migration destination")
        );
    }

    #[test]
    fn validate_snapshot_configuration_accepts_migrate_without_snapshot_dir() {
        let temp = tempdir().expect("tempdir");
        let destination = temp.path().join("dest");
        std::fs::create_dir_all(&destination).expect("create destination");

        validate_snapshot_configuration(Mode::Migrate, Some(2), &destination, None)
            .expect("config should be valid");
    }

    #[test]
    fn validate_snapshot_configuration_rejects_non_directory_snapshot_path() {
        let temp = tempdir().expect("tempdir");
        let destination = temp.path().join("dest");
        let snapshot_file = temp.path().join("snapshot-file");
        std::fs::create_dir_all(&destination).expect("create destination");
        std::fs::write(&snapshot_file, b"not-a-directory").expect("create file");

        let err = validate_snapshot_configuration(
            Mode::Migrate,
            Some(1),
            &destination,
            Some(&snapshot_file),
        )
        .expect_err("file path should be rejected");

        assert!(err.to_string().contains("must be an existing directory"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn validate_snapshot_configuration_rejects_snapshot_root_on_different_filesystem() {
        use std::os::unix::fs::MetadataExt;

        let temp = tempdir().expect("tempdir");
        let snapshot_root = temp.path().join("snapshots");
        std::fs::create_dir_all(&snapshot_root).expect("create snapshot root");
        let snapshot_dev = std::fs::metadata(&snapshot_root)
            .expect("snapshot metadata")
            .dev();

        let candidates = [
            Path::new("/proc"),
            Path::new("/sys"),
            Path::new("/dev"),
            Path::new("/tmp"),
        ];
        let destination_root = candidates
            .iter()
            .copied()
            .find(|candidate| {
                std::fs::metadata(candidate)
                    .ok()
                    .map(|meta| meta.dev() != snapshot_dev)
                    .unwrap_or(false)
            })
            .expect("expected at least one filesystem with a different device id");

        let err = validate_snapshot_configuration(
            Mode::Migrate,
            Some(1),
            destination_root,
            Some(&snapshot_root),
        )
        .expect_err("different-device snapshot root should fail");

        assert!(err.to_string().contains("same filesystem"));
    }

    #[test]
    fn process_pending_snapshots_triggers_on_deleted_batch_cadence() {
        let backend = RecordingSnapshotBackend::default();
        let temp = tempdir().expect("tempdir");
        let destination = temp.path().join("dest");
        std::fs::create_dir_all(&destination).expect("create destination");
        let mut state = build_state(&[
            ("batch-2", BatchPhase::DeleteCompleted, true),
            ("batch-1", BatchPhase::DeleteCompleted, true),
            ("batch-3", BatchPhase::SnapshotCompleted, true),
        ]);
        let mut persisted = 0usize;

        process_pending_snapshots(
            Mode::Migrate,
            Some(2),
            &destination,
            None,
            &mut state,
            &backend,
            &mut |_| {
                persisted += 1;
                Ok(())
            },
        )
        .expect("processing should succeed");

        assert_eq!(backend.calls.borrow().as_slice(), &["batch-2"]);
        assert_eq!(persisted, 1);
        assert_eq!(
            state.batch("batch-2").expect("batch exists").phase,
            BatchPhase::SnapshotCompleted
        );
    }

    #[test]
    fn process_pending_snapshots_persists_even_when_snapshot_creation_fails() {
        let backend = RecordingSnapshotBackend::default().fail_for("batch-1");
        let temp = tempdir().expect("tempdir");
        let destination = temp.path().join("dest");
        std::fs::create_dir_all(&destination).expect("create destination");
        let mut state = build_state(&[("batch-1", BatchPhase::DeleteCompleted, true)]);
        let mut persisted = 0usize;

        process_pending_snapshots(
            Mode::Migrate,
            Some(1),
            &destination,
            None,
            &mut state,
            &backend,
            &mut |_| {
                persisted += 1;
                Ok(())
            },
        )
        .expect("errors should be downgraded");

        assert_eq!(persisted, 1);
        assert_eq!(backend.calls.borrow().as_slice(), &["batch-1"]);
        assert_eq!(
            state.journal.last().expect("journal").event,
            "snapshot_failed"
        );
    }

    #[test]
    fn process_pending_snapshots_propagates_persist_failures() {
        let backend = RecordingSnapshotBackend::default();
        let temp = tempdir().expect("tempdir");
        let destination = temp.path().join("dest");
        std::fs::create_dir_all(&destination).expect("create destination");
        let mut state = build_state(&[("batch-1", BatchPhase::DeleteCompleted, true)]);

        let err = process_pending_snapshots(
            Mode::Migrate,
            Some(1),
            &destination,
            None,
            &mut state,
            &backend,
            &mut |_| Err(CaravanError::Io("persist failed".to_string())),
        )
        .expect_err("persist errors must propagate");

        assert!(err.to_string().contains("persist failed"));
    }

    #[test]
    fn resolve_existing_path_returns_nearest_existing_ancestor() {
        let temp = tempdir().expect("tempdir");
        let existing = temp.path().join("existing");
        std::fs::create_dir_all(&existing).expect("create existing");
        let missing_leaf = existing.join("a").join("b").join("c");

        let resolved = resolve_existing_path(&missing_leaf).expect("should resolve");
        assert_eq!(resolved, existing);
    }

    #[test]
    fn path_within_or_equal_matches_equal_and_descendant_paths() {
        let ancestor = Path::new("/tmp/root");
        assert!(path_within_or_equal(Path::new("/tmp/root"), ancestor));
        assert!(path_within_or_equal(Path::new("/tmp/root/child"), ancestor));
        assert!(!path_within_or_equal(Path::new("/tmp/other"), ancestor));
    }

    #[test]
    fn canonical_path_for_maybe_missing_returns_absolute_path_for_missing_relative() {
        let temp = tempdir().expect("tempdir");
        let cwd_before = std::env::current_dir().expect("cwd");
        struct CwdGuard(PathBuf);
        impl Drop for CwdGuard {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _guard = CwdGuard(cwd_before);
        std::env::set_current_dir(temp.path()).expect("set cwd");

        let resolved =
            canonical_path_for_maybe_missing(Path::new("does-not-exist")).expect("resolve path");
        assert!(resolved.is_absolute());
        assert!(resolved.ends_with("does-not-exist"));
    }

    #[test]
    fn canonical_path_rejects_missing_paths() {
        let temp = tempdir().expect("tempdir");
        let missing = temp.path().join("does-not-exist");
        let err = canonical_path(&missing).expect_err("missing path should fail canonicalization");
        assert!(err.to_string().contains("is not accessible"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn system_snapshot_backend_returns_error_when_snapshot_command_fails() {
        let backend = SystemSnapshotBackend;
        let temp = tempdir().expect("tempdir");
        let destination = temp.path().join("dest");
        std::fs::create_dir_all(&destination).expect("create destination");

        let err = backend
            .create_snapshot(&destination, None, "batch-1")
            .expect_err("backend should surface btrfs snapshot errors");
        let text = err.to_string();
        assert!(
            text.contains("failed to execute btrfs snapshot command")
                || text.contains("btrfs snapshot command failed"),
            "unexpected error: {text}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn create_btrfs_snapshot_returns_error_on_non_subvolume_destination() {
        let temp = tempdir().expect("tempdir");
        let destination = temp.path().join("dest");
        std::fs::create_dir_all(&destination).expect("create destination");

        let err = create_btrfs_snapshot(&destination, None, "batch-2")
            .expect_err("snapshot command should fail on non-btrfs destination");
        let text = err.to_string();
        assert!(
            text.contains("failed to execute btrfs snapshot command")
                || text.contains("btrfs snapshot command failed"),
            "unexpected error: {text}"
        );
    }
}
