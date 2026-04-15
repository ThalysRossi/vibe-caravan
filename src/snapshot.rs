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

pub fn snapshot_if_needed(
    mode: Mode,
    snapshot_every: Option<u32>,
    completed_batch_count: u32,
    batch_id: &str,
    destination_root: &Path,
    snapshot_root: Option<&Path>,
    state: &mut MigrationState,
    backend: &dyn SnapshotBackend,
) -> Result<Option<String>, CaravanError> {
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
            ))
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
            mode.clone(),
            snapshot_every,
            deleted_batch_count,
            &batch_id,
            destination_root,
            snapshot_root,
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

#[cfg(not(target_os = "linux"))]
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
