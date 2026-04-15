use std::path::PathBuf;

use crate::config::{Mode, TransferConfig};
use crate::error::CaravanError;
use crate::models;
use crate::models::state::{BatchPhase, BatchState, MigrationPhase, MigrationState};
use crate::plan::PlanOptions;
use crate::prompt::PromptBackend;
use crate::signal::{check_shutdown, install_signal_handlers, ShutdownFlag};
use crate::{format, migration_registry, plan, prompt, state_store, transfer, verify};

use super::shared::{
    approve_and_delete_verified_batches, ensure_destination_capacity,
    ensure_no_operator_review_blocks, persist_state_both_locations,
};

pub(super) fn execute_transfer(config: TransferConfig) -> Result<(), CaravanError> {
    // Initialize shutdown flag and install signal handlers
    let shutdown_flag = ShutdownFlag::new();
    install_signal_handlers(&shutdown_flag)?;

    // Determine state file paths:
    // 1. Primary: source directory (for resume feature)
    // 2. Secondary: current directory (for backward compatibility)
    let state_path = migration_registry::state_file_in_source(&config.source, &config.dest);
    let secondary_state_path = PathBuf::from(".caravan/state.json");

    // Update migration registry
    let registry_path = migration_registry::default_registry_path();
    let mut registry = migration_registry::MigrationRegistry::load(&registry_path)?;

    let state_filename = migration_registry::generate_state_filename(
        &config.source.to_string_lossy(),
        &config.dest.to_string_lossy(),
    );

    // Check if there's an existing incomplete migration for the same source/destination/mode
    let source_str = config.source.to_string_lossy();
    let dest_str = config.dest.to_string_lossy();
    let mode_str = if config.mode == Mode::Staging {
        "staging"
    } else {
        "migrate"
    };

    let migration_id = if let Some(existing_migration) =
        registry.find_by_source_dest(&source_str, &dest_str, mode_str)
    {
        println!("Resuming existing migration ID: {}", existing_migration.id);
        existing_migration.id
    } else {
        // No existing migration found, create a new one
        let new_id = registry.add_migration(&source_str, &dest_str, mode_str, &state_filename);
        println!("Migration registered with new ID: {}", new_id);
        new_id
    };

    registry.update_status(migration_id, migration_registry::MigrationStatus::Running)?;
    registry.save(&registry_path)?;

    // Try to load existing state file, or create new state
    let mut state = if state_path.exists() {
        // Load existing state
        let loaded_state = state_store::load_state(&state_path)?;
        println!("Loaded existing state from: {}", state_path.display());
        loaded_state
    } else {
        // No existing state file, create new state
        // Check if source directory is writable before creating new state
        migration_registry::check_source_writable(&config.source)?;
        MigrationState::new(mode_str, &source_str, &dest_str)
    };

    state.batch_size_bytes = config.batch_size_bytes;
    state.max_files = config.max_files;
    state.snapshot_every = config.snapshot_every;
    state.verification_mode = config.verification.clone();
    state.copy_buffer_size = config.copy_buffer_size;
    state.buffered_copy_threshold = config.buffered_copy_threshold;

    // Fail closed before any phase work if persisted state requires operator review.
    ensure_no_operator_review_blocks(&state)?;

    println!(
        "State will be saved to: {} (primary) and {} (backward compatibility)",
        state_path.display(),
        secondary_state_path.display()
    );

    // Build plan
    let plan_opts = PlanOptions {
        batch_size_bytes: config.batch_size_bytes,
        max_files: config.max_files.map(|v| v as usize),
    };
    let plan = plan::build_plan(&config.source, &plan_opts)?;

    println!(
        "Planned {} batches for {} files ({} total)",
        plan.batches.len(),
        plan.source_file_count,
        format::format_bytes(plan.source_total_bytes)
    );

    // Add ALL batches to state upfront BEFORE processing any
    // Only add batches that don't already exist in state to preserve existing progress
    for batch in &plan.batches {
        if state.batch(&batch.id).is_none() {
            state.upsert_batch(BatchState {
                batch_id: batch.id.clone(),
                phase: BatchPhase::Planned,
                verification_passed: false,
                approved_for_delete: false,
                deleted: false,
            });
        }
        // If batch already exists (e.g., from a previous run), keep its current state
    }
    persist_state_both_locations(&state_path, &secondary_state_path, &state)?;

    let copy_backend = transfer::LocalFsCopyBackend::with_config(
        config.copy_buffer_size,
        config.buffered_copy_threshold,
    );

    // Hardware-aware warnings for inappropriate buffer sizes
    {
        let buffer_size_mb = config.copy_buffer_size as f64 / (1024.0 * 1024.0);
        let threshold_mb = config.buffered_copy_threshold as f64 / (1024.0 * 1024.0);

        // Warn about small buffer sizes (common mistake when migrating from SSD to HDD)
        if buffer_size_mb < 4.0 {
            eprintln!("[WARNING] Copy buffer size is small ({:.2} MiB). For HDD performance, consider using at least 16 MiB buffer size.", buffer_size_mb);
            eprintln!("  Use --copy-buffer-size 16MiB to optimize for 5400-7200 RPM HDDs.");
        }

        // Warn about inappropriate thresholds
        if threshold_mb < 1.0 {
            eprintln!("[WARNING] Buffered copy threshold is very small ({:.2} MiB). OS copy is more efficient for files smaller than 8 MiB.", threshold_mb);
            eprintln!(
                "  Consider using --buffered-copy-threshold 8MiB for better HDD performance."
            );
        }

        // Debug info about current configuration
        eprintln!(
            "[DEBUG] Using copy buffer size: {:.2} MiB, buffered copy threshold: {:.2} MiB",
            buffer_size_mb, threshold_mb
        );
    }

    let mut processed_batches = 0_u32;

    // === PHASE 1: COPY ALL BATCHES ===
    state.migration_phase = MigrationPhase::Copying;
    persist_state_both_locations(&state_path, &secondary_state_path, &state)?;
    println!("\n=== Copying all batches ===");

    for batch in &plan.batches {
        check_shutdown(&shutdown_flag)?;

        // Skip batches that are already deleted or already copied
        if let Some(existing_batch) = state.batch(&batch.id) {
            if existing_batch.deleted {
                println!("Skipping {}: already completed", batch.id);
                processed_batches += 1;
                continue;
            }
            if matches!(
                existing_batch.phase,
                BatchPhase::CopyCompleted
                    | BatchPhase::VerifyCompleted
                    | BatchPhase::ApprovedForDelete
                    | BatchPhase::DeleteCompleted
                    | BatchPhase::SnapshotCompleted
            ) {
                println!(
                    "Skipping {}: copy already completed (phase: {:?})",
                    batch.id, existing_batch.phase
                );
                continue;
            }
        }

        println!(
            "\n=== Copying {} ({} files, {}) ===",
            batch.id,
            batch.file_count,
            format::format_bytes(batch.total_bytes)
        );

        // Initialize batch state if not present
        let mut batch_state = state
            .batch(&batch.id)
            .cloned()
            .unwrap_or_else(|| BatchState {
                batch_id: batch.id.clone(),
                phase: BatchPhase::Planned,
                verification_passed: false,
                approved_for_delete: false,
                deleted: false,
            });

        ensure_destination_capacity(&config.dest, batch.total_bytes)?;

        // Check for naming conflicts
        let conflict_report = crate::conflict::detect_batch_conflicts(batch, &config.dest)?;
        if conflict_report.has_conflicts {
            // Determine whether to skip this batch
            let should_skip = if config.skip_conflicts || !config.interactive {
                // Auto-skip in non-interactive mode or when skip_conflicts flag is set
                true
            } else {
                // Interactive mode: ask user
                let prompt_backend = prompt::InteractivePrompt;
                prompt_backend.confirm_conflict_skip(&batch.id, &conflict_report)?
            };

            if should_skip {
                println!(
                    "⚠️  Skipping batch '{}' due to {} naming conflict(s)",
                    batch.id, conflict_report.total_conflicts
                );

                // Mark batch as failed so later phases do not treat it as copied.
                // A later rerun can retry after the operator resolves the conflict.
                batch_state.phase = BatchPhase::Failed;
                batch_state.verification_passed = false;
                state.upsert_batch(batch_state.clone());
                persist_state_both_locations(&state_path, &secondary_state_path, &state)?;

                // Skip to next batch
                continue;
            }
            // If user chooses not to skip, we'll continue with copy (overwrites files)
            // This is the MVP - we only have skip functionality for now
            // In future implementations, we could proceed with overwrite
        }

        // Copy batch
        batch_state.phase = BatchPhase::CopyStarted;
        state.upsert_batch(batch_state.clone());
        persist_state_both_locations(&state_path, &secondary_state_path, &state)?;

        let mut progress = crate::progress::TerminalProgress::new();
        transfer::transfer_batch_with_progress(
            batch,
            &config.source,
            &config.dest,
            &copy_backend,
            &mut progress,
        )?;

        batch_state.phase = BatchPhase::CopyCompleted;
        batch_state.verification_passed = false;
        state.upsert_batch(batch_state.clone());
        persist_state_both_locations(&state_path, &secondary_state_path, &state)?;
    }

    // === PHASE 2: VERIFY ALL BATCHES ===
    state.migration_phase = MigrationPhase::Verifying;
    persist_state_both_locations(&state_path, &secondary_state_path, &state)?;
    println!("\n=== Verifying all batches ===");

    for batch in &plan.batches {
        check_shutdown(&shutdown_flag)?;

        // Skip batches already verified or deleted
        if let Some(existing_batch) = state.batch(&batch.id) {
            if existing_batch.deleted {
                continue;
            }
            if existing_batch.phase == BatchPhase::Failed {
                println!(
                    "Skipping {}: requires operator review before verification",
                    batch.id
                );
                continue;
            }
            if existing_batch.verification_passed
                && matches!(
                    existing_batch.phase,
                    BatchPhase::VerifyCompleted
                        | BatchPhase::ApprovedForDelete
                        | BatchPhase::DeleteCompleted
                        | BatchPhase::SnapshotCompleted
                )
            {
                println!(
                    "Skipping {}: verification already completed (phase: {:?})",
                    batch.id, existing_batch.phase
                );
                processed_batches += 1;
                continue;
            }
        }

        println!(
            "\n=== Verifying {} ({} files, {}) ===",
            batch.id,
            batch.file_count,
            format::format_bytes(batch.total_bytes)
        );

        let mut batch_state = state.batch(&batch.id).cloned().ok_or_else(|| {
            CaravanError::InvalidArguments(format!(
                "missing batch state for {} before verification",
                batch.id
            ))
        })?;

        // Verify batch
        let mut progress = crate::progress::TerminalProgress::new();
        let verification_report = verify::verify_batch_with_progress(
            batch,
            &config.source,
            &config.dest,
            config.verification.clone(),
            &mut progress,
        )?;

        batch_state.phase = BatchPhase::VerifyCompleted;
        batch_state.verification_passed =
            verification_report.status == models::verification::VerificationStatus::Pass;
        state.upsert_batch(batch_state.clone());
        persist_state_both_locations(&state_path, &secondary_state_path, &state)?;

        if !batch_state.verification_passed {
            eprintln!(
                "Verification failed: {}",
                verification_report.recommended_action
            );
            eprintln!("Missing: {:?}", verification_report.missing_files);
            eprintln!("Mismatched: {:?}", verification_report.mismatched_files);
            return Err(CaravanError::InvalidArguments(
                "verification failed".to_string(),
            ));
        }

        println!("Verification passed!");
        processed_batches += 1;
    }

    // Check for shutdown before proceeding to deletion phase
    check_shutdown(&shutdown_flag)?;

    ensure_no_operator_review_blocks(&state)?;

    let mut persist_state = |current_state: &MigrationState| {
        persist_state_both_locations(&state_path, &secondary_state_path, current_state)
    };
    approve_and_delete_verified_batches(
        &mut state,
        &config.source,
        config.interactive,
        &shutdown_flag,
        &mut persist_state,
        "execute_transfer",
    )?;

    // Count completed batches (including previously deleted ones)
    let completed_count = state.batches.iter().filter(|b| b.deleted).count();
    println!(
        "\n=== Migration complete! {} batches processed, {} total completed ===",
        processed_batches, completed_count
    );
    Ok(())
}
