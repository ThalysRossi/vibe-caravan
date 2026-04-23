use std::path::Path;

use crate::config::{Mode, TransferConfig};
use crate::error::CaravanError;
use crate::migration_registry;
use crate::models::state::{BatchPhase, BatchState, MigrationState};
use crate::plan::PlanningSnapshot;
use crate::platform::is_windows_build;
use crate::state_store;

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

        let loaded_path = match loaded.source {
            state_store::ReconciledStateSource::Primary => state_path,
            state_store::ReconciledStateSource::Secondary => secondary_state_path,
        };
        println!("Loaded existing state from: {}", loaded_path.display());
        return Ok(loaded.state);
    }

    migration_registry::check_source_writable(&config.source)?;
    Ok(MigrationState::new(mode, source, dest))
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
    let resolved_strategy =
        crate::transfer::resolve_copy_strategy(config.copy_strategy, &config.mode);
    let can_use_buffered_path = matches!(
        resolved_strategy,
        crate::transfer::ResolvedCopyStrategy::Buffered
            | crate::transfer::ResolvedCopyStrategy::Hybrid
    ) || (!is_windows_build()
        && matches!(
            resolved_strategy,
            crate::transfer::ResolvedCopyStrategy::NativePreferred
        ));
    let threshold_applies = matches!(
        resolved_strategy,
        crate::transfer::ResolvedCopyStrategy::Hybrid
    ) || (!is_windows_build()
        && matches!(
            resolved_strategy,
            crate::transfer::ResolvedCopyStrategy::NativePreferred
        ));

    if can_use_buffered_path && buffer_size_mb < 4.0 {
        eprintln!(
            "[WARNING] Copy buffer size is small ({:.2} MiB). For HDD performance, consider using at least 16 MiB buffer size.",
            buffer_size_mb
        );
        eprintln!("  Use --copy-buffer-size 16MiB to optimize for 5400-7200 RPM HDDs.");
    }

    if threshold_applies && threshold_mb < 1.0 {
        eprintln!(
            "[WARNING] Buffered copy threshold is very small ({:.2} MiB). OS copy is more efficient for files smaller than 8 MiB.",
            threshold_mb
        );
        eprintln!("  Consider using --buffered-copy-threshold 8MiB for better HDD performance.");
    }

    if !is_windows_build() && config.copy_strategy == crate::config::CopyStrategy::Native {
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
