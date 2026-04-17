use std::path::Path;

use crate::config::{Mode, TransferConfig};
use crate::error::CaravanError;
use crate::migration_registry;
use crate::models::state::{BatchPhase, BatchState, MigrationState};
use crate::plan::PlanningSnapshot;
use crate::state_store;

pub(super) fn mode_name(mode: &Mode) -> &'static str {
    match mode {
        Mode::Staging => "staging",
        Mode::Migrate => "migrate",
    }
}

pub(super) fn register_migration(
    source: &str,
    dest: &str,
    mode: &str,
    state_filename: &str,
) -> Result<u64, CaravanError> {
    let registry_path = migration_registry::default_registry_path();
    let mut registry = migration_registry::MigrationRegistry::load(&registry_path)?;

    let migration_id =
        if let Some(existing_migration) = registry.find_by_source_dest(source, dest, mode) {
            println!("Resuming existing migration ID: {}", existing_migration.id);
            existing_migration.id
        } else {
            let new_id = registry.add_migration(source, dest, mode, state_filename);
            println!("Migration registered with new ID: {}", new_id);
            new_id
        };

    registry.save(&registry_path)?;
    migration_registry::persist_status_transition_with_intent(
        &registry_path,
        migration_id,
        migration_registry::MigrationStatus::Running,
    )?;
    Ok(migration_id)
}

pub(super) fn set_migration_status(
    migration_id: u64,
    status: migration_registry::MigrationStatus,
) -> Result<(), CaravanError> {
    let registry_path = migration_registry::default_registry_path();
    migration_registry::persist_status_transition_with_intent(&registry_path, migration_id, status)
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
        let loaded_state =
            state_store::load_state_with_compat_reconciliation(state_path, secondary_state_path)?;
        println!("Loaded existing state from: {}", state_path.display());
        return Ok(loaded_state);
    }

    migration_registry::check_source_writable(&config.source)?;
    Ok(MigrationState::new(mode, source, dest))
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
    state.copy_buffer_size = config.copy_buffer_size;
    state.buffered_copy_threshold = config.buffered_copy_threshold;
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
    let buffer_size_mb = config.copy_buffer_size as f64 / (1024.0 * 1024.0);
    let threshold_mb = config.buffered_copy_threshold as f64 / (1024.0 * 1024.0);

    if buffer_size_mb < 4.0 {
        eprintln!(
            "[WARNING] Copy buffer size is small ({:.2} MiB). For HDD performance, consider using at least 16 MiB buffer size.",
            buffer_size_mb
        );
        eprintln!("  Use --copy-buffer-size 16MiB to optimize for 5400-7200 RPM HDDs.");
    }

    if threshold_mb < 1.0 {
        eprintln!(
            "[WARNING] Buffered copy threshold is very small ({:.2} MiB). OS copy is more efficient for files smaller than 8 MiB.",
            threshold_mb
        );
        eprintln!("  Consider using --buffered-copy-threshold 8MiB for better HDD performance.");
    }

    if !cfg!(windows) && config.copy_strategy == crate::config::CopyStrategy::Native {
        eprintln!(
            "[WARNING] Native copy strategy was requested on a non-Windows platform; caravan will fall back to hybrid copy."
        );
    }

    tracing::debug!(
        copy_strategy = ?config.copy_strategy,
        copy_buffer_size_mib = buffer_size_mb,
        buffered_copy_threshold_mib = threshold_mb,
        "Using copy backend configuration"
    );
}
