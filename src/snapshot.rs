use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Mode;
use crate::error::CaravanError;
use crate::models::state::{BatchPhase, JournalEntry, MigrationState};

pub trait SnapshotBackend {
    fn create_snapshot(
        &self,
        destination_root: &Path,
        snapshot_root: Option<&Path>,
        batch_id: &str,
    ) -> Result<String, CaravanError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemSnapshotBackend;

impl SnapshotBackend for SystemSnapshotBackend {
    fn create_snapshot(
        &self,
        destination_root: &Path,
        snapshot_root: Option<&Path>,
        batch_id: &str,
    ) -> Result<String, CaravanError> {
        create_btrfs_snapshot(destination_root, snapshot_root, batch_id)
    }
}

pub struct SnapshotRequest<'a> {
    pub mode: Mode,
    pub snapshot_every: Option<u32>,
    pub completed_batch_count: u32,
    pub batch_id: &'a str,
    pub destination_root: &'a Path,
    pub snapshot_root: Option<&'a Path>,
}

pub fn snapshot_if_needed(
    request: SnapshotRequest<'_>,
    state: &mut MigrationState,
    backend: &dyn SnapshotBackend,
) -> Result<Option<String>, CaravanError> {
    let SnapshotRequest {
        mode,
        snapshot_every,
        completed_batch_count,
        batch_id,
        destination_root,
        snapshot_root,
    } = request;

    if mode == Mode::Staging {
        if snapshot_every.is_some() {
            return Err(CaravanError::InvalidArguments(
                "snapshots are only supported in migrate mode".to_string(),
            ));
        }
        return Ok(None);
    }

    let cadence = match snapshot_every {
        Some(value) if value > 0 => value,
        Some(_) => {
            return Err(CaravanError::InvalidArguments(
                "snapshot cadence must be greater than zero".to_string(),
            ));
        }
        None => return Ok(None),
    };

    if completed_batch_count == 0 || completed_batch_count % cadence != 0 {
        return Ok(None);
    }

    match backend.create_snapshot(destination_root, snapshot_root, batch_id) {
        Ok(snapshot_name) => {
            state.last_successful_snapshot_name = Some(snapshot_name.clone());
            if let Some(batch) = state.batches.iter_mut().find(|b| b.batch_id == batch_id) {
                batch.phase = BatchPhase::SnapshotCompleted;
            }
            state.journal.push(JournalEntry {
                event: "snapshot_completed".to_string(),
                batch_id: batch_id.to_string(),
                timestamp_unix_secs: now_unix_secs(),
                context: snapshot_name.clone(),
            });
            Ok(Some(snapshot_name))
        }
        Err(err) => {
            state.journal.push(JournalEntry {
                event: "snapshot_failed".to_string(),
                batch_id: batch_id.to_string(),
                timestamp_unix_secs: now_unix_secs(),
                context: err.to_string(),
            });
            Err(err)
        }
    }
}

pub fn validate_snapshot_configuration(
    mode: Mode,
    snapshot_every: Option<u32>,
    destination_root: &Path,
    snapshot_root: Option<&Path>,
) -> Result<(), CaravanError> {
    if snapshot_root.is_some() && snapshot_every.is_none() {
        return Err(CaravanError::InvalidArguments(
            "snapshot-dir requires snapshot-every".to_string(),
        ));
    }

    if mode == Mode::Staging {
        if snapshot_every.is_some() || snapshot_root.is_some() {
            return Err(CaravanError::InvalidArguments(
                "snapshots are only supported in migrate mode".to_string(),
            ));
        }
        return Ok(());
    }

    if let Some(value) = snapshot_every {
        if value == 0 {
            return Err(CaravanError::InvalidArguments(
                "snapshot cadence must be greater than zero".to_string(),
            ));
        }
    } else {
        return Ok(());
    }

    let Some(snapshot_root) = snapshot_root else {
        return Ok(());
    };
    let snapshot_check_path = canonical_path(snapshot_root)?;
    let destination_check_path = canonical_path_for_maybe_missing(destination_root)?;
    if path_within_or_equal(&snapshot_check_path, &destination_check_path) {
        return Err(CaravanError::InvalidArguments(format!(
            "snapshot destination must not be inside migration destination: snapshot='{}', destination='{}'",
            snapshot_root.display(),
            destination_root.display()
        )));
    }

    let snapshot_meta = std::fs::metadata(snapshot_root).map_err(|err| {
        CaravanError::InvalidArguments(format!(
            "snapshot destination '{}' is not accessible: {}",
            snapshot_root.display(),
            err
        ))
    })?;
    if !snapshot_meta.is_dir() {
        return Err(CaravanError::InvalidArguments(format!(
            "snapshot destination must be an existing directory: '{}'",
            snapshot_root.display()
        )));
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let destination_probe = resolve_existing_path(destination_root)?;
        let destination_meta = std::fs::metadata(&destination_probe).map_err(|err| {
            CaravanError::InvalidArguments(format!(
                "destination '{}' is not accessible for snapshot checks: {}",
                destination_probe.display(),
                err
            ))
        })?;
        if snapshot_meta.dev() != destination_meta.dev() {
            return Err(CaravanError::InvalidArguments(format!(
                "snapshot destination '{}' must be on the same filesystem as destination '{}'",
                snapshot_root.display(),
                destination_root.display()
            )));
        }
    }

    Ok(())
}

pub fn process_pending_snapshots(
    mode: Mode,
    snapshot_every: Option<u32>,
    destination_root: &Path,
    snapshot_root: Option<&Path>,
    state: &mut MigrationState,
    backend: &dyn SnapshotBackend,
    persist_state: &mut dyn FnMut(&MigrationState) -> Result<(), CaravanError>,
) -> Result<(), CaravanError> {
    validate_snapshot_configuration(
        mode.clone(),
        snapshot_every,
        destination_root,
        snapshot_root,
    )?;

    if snapshot_every.is_none() {
        return Ok(());
    }

    let mut deleted_batch_ids: Vec<String> = state
        .batches
        .iter()
        .filter(|batch| batch.deleted)
        .map(|batch| batch.batch_id.clone())
        .collect();
    deleted_batch_ids.sort();

    let mut deleted_batch_count = 0u32;
    for batch_id in deleted_batch_ids {
        deleted_batch_count = deleted_batch_count.saturating_add(1);

        if state
            .batch(&batch_id)
            .map(|batch| batch.phase == BatchPhase::SnapshotCompleted)
            .unwrap_or(false)
        {
            continue;
        }

        match snapshot_if_needed(
            SnapshotRequest {
                mode: mode.clone(),
                snapshot_every,
                completed_batch_count: deleted_batch_count,
                batch_id: &batch_id,
                destination_root,
                snapshot_root,
            },
            state,
            backend,
        ) {
            Ok(Some(snapshot_name)) => {
                eprintln!(
                    "[SNAPSHOT] Created '{}' after deleted batch count {} (batch={})",
                    snapshot_name, deleted_batch_count, batch_id
                );
                persist_state(state)?;
            }
            Ok(None) => {}
            Err(err) => {
                eprintln!(
                    "[WARNING] Snapshot failed for batch {} after deleted batch count {}: {}",
                    batch_id, deleted_batch_count, err
                );
                persist_state(state)?;
            }
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn create_btrfs_snapshot(
    destination_root: &Path,
    snapshot_root: Option<&Path>,
    batch_id: &str,
) -> Result<String, CaravanError> {
    use std::process::Command;

    let snapshot_name = format!("caravan-snap-{}-{}", now_unix_secs(), batch_id);
    let snapshot_parent =
        snapshot_root.unwrap_or_else(|| destination_root.parent().unwrap_or(destination_root));
    let snapshot_path = snapshot_parent.join(&snapshot_name);

    let output = Command::new("btrfs")
        .args(["subvolume", "snapshot", "-r"])
        .arg(destination_root)
        .arg(&snapshot_path)
        .output()
        .map_err(|err| {
            CaravanError::InvalidArguments(format!(
                "failed to execute btrfs snapshot command for {}: {}",
                destination_root.display(),
                err
            ))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CaravanError::InvalidArguments(format!(
            "btrfs snapshot command failed for {} -> {}: {}",
            destination_root.display(),
            snapshot_path.display(),
            stderr.trim()
        )));
    }

    Ok(snapshot_name)
}

#[cfg(target_os = "windows")]
fn create_btrfs_snapshot(
    _destination_root: &Path,
    _snapshot_root: Option<&Path>,
    _batch_id: &str,
) -> Result<String, CaravanError> {
    Err(CaravanError::InvalidArguments(
        "btrfs snapshots are only supported on Linux".to_string(),
    ))
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn resolve_existing_path(path: &Path) -> Result<PathBuf, CaravanError> {
    let mut candidate: Option<&Path> = Some(path);
    while let Some(current) = candidate {
        if current.exists() {
            return Ok(current.to_path_buf());
        }
        candidate = current.parent();
    }

    Err(CaravanError::InvalidArguments(format!(
        "path '{}' and its parents do not exist",
        path.display()
    )))
}

fn path_within_or_equal(candidate: &Path, ancestor: &Path) -> bool {
    candidate == ancestor || candidate.starts_with(ancestor)
}

fn canonical_path(path: &Path) -> Result<PathBuf, CaravanError> {
    std::fs::canonicalize(path).map_err(|err| {
        CaravanError::InvalidArguments(format!(
            "path '{}' is not accessible: {}",
            path.display(),
            err
        ))
    })
}

fn canonical_path_for_maybe_missing(path: &Path) -> Result<PathBuf, CaravanError> {
    if path.exists() {
        return canonical_path(path);
    }

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|err| {
                CaravanError::InvalidArguments(format!(
                    "failed to read current directory while resolving '{}': {}",
                    path.display(),
                    err
                ))
            })?
            .join(path)
    };
    Ok(absolute)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashSet;

    use tempfile::tempdir;

    use crate::models::state::{BatchState, MigrationPhase};

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
}
