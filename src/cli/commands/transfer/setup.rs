use std::path::Path;

use crate::config::{Mode, TransferConfig};
use crate::error::CaravanError;
use crate::migration_registry;
use crate::models::state::{BatchPhase, BatchState, MigrationState};
use crate::plan::PlanningSnapshot;
use crate::state_store;
use crate::transfer::normalize_legacy_copy_strategy;

use super::super::shared::AppContext;

pub(super) fn mode_name(mode: &Mode) -> &'static str {
    match mode {
        Mode::Staging => "staging",
        Mode::Migrate => "migrate",
    }
}

pub(super) fn register_migration(
    app_context: &AppContext,
    source: &str,
    dest: &str,
    mode: &str,
    state_filename: &str,
) -> Result<u64, CaravanError> {
    let (migration_id, created) =
        app_context.register_or_reuse_migration(source, dest, mode, state_filename)?;
    if created {
        println!("Migration registered with new ID: {}", migration_id);
    } else {
        println!("Resuming existing migration ID: {}", migration_id);
    }
    app_context
        .persist_migration_status(migration_id, migration_registry::MigrationStatus::Running)?;
    Ok(migration_id)
}

pub(super) fn load_or_create_state(
    config: &TransferConfig,
    state_path: &Path,
    secondary_state_path: &Path,
    mode: &str,
    source: &str,
    dest: &str,
) -> Result<MigrationState, CaravanError> {
    if state_path.exists() || secondary_state_path.exists() {
        let loaded = state_store::load_state_with_compat_reconciliation_detailed(
            state_path,
            secondary_state_path,
        )?;
        if !state_identity_matches(&loaded.state, mode, source, dest) {
            return handle_state_identity_mismatch(
                config,
                mode,
                source,
                dest,
                state_path,
                secondary_state_path,
                loaded.source,
                &loaded.state,
            );
        }

        let mut loaded_state = loaded.state;
        normalize_legacy_state_copy_strategy(&mut loaded_state);

        let loaded_path = match loaded.source {
            state_store::ReconciledStateSource::Primary => state_path,
            state_store::ReconciledStateSource::Secondary => secondary_state_path,
        };
        println!("Loaded existing state from: {}", loaded_path.display());
        return Ok(loaded_state);
    }

    migration_registry::check_source_writable(&config.source)?;
    Ok(MigrationState::new(mode, source, dest))
}

fn normalize_legacy_state_copy_strategy(state: &mut MigrationState) {
    state.copy_strategy = normalize_legacy_copy_strategy(state.copy_strategy);
}

fn normalize_identity_path(path: &str) -> String {
    let as_path = std::path::Path::new(path);
    let normalized_path = std::fs::canonicalize(as_path).unwrap_or_else(|_| as_path.to_path_buf());
    let normalized = normalized_path.to_string_lossy().replace('\\', "/");
    #[cfg(target_os = "windows")]
    {
        return normalized.to_ascii_lowercase();
    }
    #[cfg(target_os = "linux")]
    {
        normalized
    }
}

fn state_identity_matches(state: &MigrationState, mode: &str, source: &str, dest: &str) -> bool {
    state.mode.eq_ignore_ascii_case(mode)
        && normalize_identity_path(&state.source) == normalize_identity_path(source)
        && normalize_identity_path(&state.destination) == normalize_identity_path(dest)
}

fn identity_mismatch_error(
    loaded_state: &MigrationState,
    mode: &str,
    source: &str,
    dest: &str,
) -> CaravanError {
    CaravanError::InvalidArguments(format!(
        "state identity mismatch: loaded state references mode='{}' source='{}' destination='{}' but current command is mode='{}' source='{}' destination='{}'",
        loaded_state.mode, loaded_state.source, loaded_state.destination, mode, source, dest
    ))
}

fn handle_state_identity_mismatch(
    config: &TransferConfig,
    mode: &str,
    source: &str,
    dest: &str,
    state_path: &Path,
    secondary_state_path: &Path,
    loaded_from: state_store::ReconciledStateSource,
    loaded_state: &MigrationState,
) -> Result<MigrationState, CaravanError> {
    match loaded_from {
        state_store::ReconciledStateSource::Primary => {
            eprintln!(
                "[ERROR] canonical state {} does not match current migration identity.",
                state_path.display()
            );
            Err(identity_mismatch_error(loaded_state, mode, source, dest))
        }
        state_store::ReconciledStateSource::Secondary => {
            eprintln!(
                "[WARNING] compatibility backup state {} does not match current migration identity; ignoring backup and creating a new state.",
                secondary_state_path.display()
            );
            migration_registry::check_source_writable(&config.source)?;
            Ok(MigrationState::new(mode, source, dest))
        }
    }
}

pub(super) fn apply_transfer_config(state: &mut MigrationState, config: &TransferConfig) {
    state.batch_size_bytes = config.batch_size_bytes;
    state.max_files = config.max_files;
    state.snapshot_every = config.snapshot_every;
    state.snapshot_dir = config
        .snapshot_dir
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    state.copy_strategy = config.copy_strategy;
}

pub(super) fn planned_batch_state(batch_id: &str) -> BatchState {
    BatchState {
        batch_id: batch_id.to_string(),
        phase: BatchPhase::Planned,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    }
}

pub(super) fn seed_state_batches(state: &mut MigrationState, plan: &PlanningSnapshot) {
    for batch in &plan.batches {
        if state.planned_batch(&batch.id).is_none() {
            state.upsert_planned_batch(crate::models::state::PlannedBatch::from_batch(batch));
        }

        if state.batch(&batch.id).is_none() {
            state.upsert_batch(planned_batch_state(&batch.id));
        }
    }
}

pub(super) fn warn_copy_backend_config(config: &TransferConfig) {
    tracing::debug!(
        copy_strategy = ?config.copy_strategy,
        "Using copy backend configuration"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CopyStrategy;
    use crate::models::batch::Batch;
    use crate::models::file_entry::FileEntry;
    use crate::models::state::MigrationPhase;
    use crate::state_store::ReconciledStateSource;
    use std::path::PathBuf;

    fn sample_transfer_config(source: &std::path::Path) -> TransferConfig {
        TransferConfig {
            mode: Mode::Staging,
            source: source.to_path_buf(),
            dest: PathBuf::from("/dest"),
            batch_size_bytes: 1024,
            max_files: Some(10),
            snapshot_every: Some(2),
            snapshot_dir: Some(PathBuf::from("/snapshots")),
            interactive: true,
            log_level: "info".to_string(),
            skip_conflicts: false,
            conflict_policy: crate::config::ConflictPolicy::SkipFile,
            recover_failed: false,
            allow_unsafe_filesystems: false,
            copy_strategy: CopyStrategy::Auto,
        }
    }

    fn sample_plan() -> PlanningSnapshot {
        PlanningSnapshot {
            source_file_count: 1,
            source_total_bytes: 5,
            batches: vec![Batch {
                id: "batch-000001".to_string(),
                file_count: 1,
                total_bytes: 5,
                files: vec![FileEntry {
                    relative_path: PathBuf::from("a.txt"),
                    size_bytes: 5,
                    modified_time: None,
                }],
            }],
        }
    }

    #[test]
    fn mode_name_renders_expected_values() {
        assert_eq!(mode_name(&Mode::Staging), "staging");
        assert_eq!(mode_name(&Mode::Migrate), "migrate");
    }

    #[test]
    fn normalize_legacy_state_copy_strategy_handles_buffered() {
        let mut state = MigrationState::new("staging", "/src", "/dst");
        state.copy_strategy = CopyStrategy::Buffered;
        normalize_legacy_state_copy_strategy(&mut state);
        assert_eq!(state.copy_strategy, CopyStrategy::Auto);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn normalize_legacy_state_copy_strategy_handles_linux_native() {
        let mut state = MigrationState::new("staging", "/src", "/dst");
        state.copy_strategy = CopyStrategy::Native;
        normalize_legacy_state_copy_strategy(&mut state);
        assert_eq!(state.copy_strategy, CopyStrategy::Auto);
    }

    #[test]
    fn apply_transfer_config_updates_runtime_fields() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let config = sample_transfer_config(tmp.path());

        let mut state = MigrationState::new("staging", "/old-src", "/old-dst");
        apply_transfer_config(&mut state, &config);

        assert_eq!(state.batch_size_bytes, 1024);
        assert_eq!(state.max_files, Some(10));
        assert_eq!(state.snapshot_every, Some(2));
        assert_eq!(state.snapshot_dir.as_deref(), Some("/snapshots"));
        assert_eq!(state.copy_strategy, CopyStrategy::Auto);
    }

    #[test]
    fn planned_batch_state_sets_safe_defaults() {
        let batch = planned_batch_state("batch-42");
        assert_eq!(batch.batch_id, "batch-42");
        assert_eq!(batch.phase, BatchPhase::Planned);
        assert!(!batch.verification_passed);
        assert!(!batch.approved_for_delete);
        assert!(!batch.deleted);
    }

    #[test]
    fn seed_state_batches_is_idempotent() {
        let mut state = MigrationState::new("staging", "/src", "/dst");
        let plan = sample_plan();

        seed_state_batches(&mut state, &plan);
        seed_state_batches(&mut state, &plan);

        assert_eq!(state.planned_batches.len(), 1);
        assert_eq!(state.batches.len(), 1);
        assert_eq!(state.batches[0].batch_id, "batch-000001");
    }

    #[test]
    fn handle_state_identity_mismatch_secondary_creates_fresh_state() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let config = sample_transfer_config(tmp.path());
        let loaded = MigrationState::new("staging", "/wrong-src", "/wrong-dst");
        let primary = tmp.path().join("primary.json");
        let secondary = tmp.path().join("secondary.json");

        let recovered = handle_state_identity_mismatch(
            &config,
            "staging",
            "/src",
            "/dst",
            &primary,
            &secondary,
            ReconciledStateSource::Secondary,
            &loaded,
        )
        .expect("secondary mismatch should recover with fresh state");

        assert_eq!(recovered.mode, "staging");
        assert_eq!(recovered.source, "/src");
        assert_eq!(recovered.destination, "/dst");
        assert_eq!(recovered.migration_phase, MigrationPhase::NotStarted);
    }

    #[test]
    fn handle_state_identity_mismatch_primary_returns_error() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let config = sample_transfer_config(tmp.path());
        let loaded = MigrationState::new("staging", "/wrong-src", "/wrong-dst");
        let primary = tmp.path().join("primary.json");
        let secondary = tmp.path().join("secondary.json");

        let err = handle_state_identity_mismatch(
            &config,
            "staging",
            "/src",
            "/dst",
            &primary,
            &secondary,
            ReconciledStateSource::Primary,
            &loaded,
        )
        .expect_err("primary mismatch must fail closed");

        assert!(err.to_string().contains("state identity mismatch"));
    }
}
