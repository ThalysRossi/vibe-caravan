use std::path::{Path, PathBuf};

use crate::config::{Mode, TransferConfig};
use crate::error::CaravanError;
use crate::models;
use crate::models::state::{BatchPhase, BatchState, MigrationPhase, MigrationState};
use crate::plan::PlanOptions;
use crate::prompt::PromptBackend;
use crate::signal::{check_shutdown, install_signal_handlers, ShutdownFlag};
use crate::{
    capacity, cleanup, format, migration_registry, plan, prompt, resume, state_store, transfer,
    verify,
};

/// Save state to both primary (source directory) and secondary (current directory) locations
fn persist_state_both_locations(
    primary_path: &Path,
    secondary_path: &Path,
    state: &MigrationState,
) -> Result<(), CaravanError> {
    // Save to primary location (source directory)
    state_store::persist_state(primary_path, state)?;

    // Save to secondary location (current directory) for backward compatibility
    state_store::persist_state(secondary_path, state)?;

    Ok(())
}

fn ensure_destination_capacity(dest: &Path, required_bytes: u64) -> Result<(), CaravanError> {
    let capacity_report = capacity::check_capacity(dest, required_bytes, 0)?;
    if capacity_report.decision == capacity::CapacityDecision::Abort {
        eprintln!(
            "Capacity check failed: {}",
            capacity_report.reason.unwrap_or_default()
        );
        return Err(CaravanError::InvalidArguments(
            "insufficient destination space".to_string(),
        ));
    }
    Ok(())
}

fn failed_batches_requiring_review(state: &MigrationState) -> Vec<String> {
    state
        .batches
        .iter()
        .filter(|batch| batch.phase == BatchPhase::Failed && !batch.deleted)
        .map(|batch| batch.batch_id.clone())
        .collect()
}

fn failed_verification_batches_requiring_review(state: &MigrationState) -> Vec<String> {
    state
        .batches
        .iter()
        .filter(|batch| {
            !batch.deleted
                && !batch.verification_passed
                && matches!(
                    batch.phase,
                    BatchPhase::VerifyCompleted
                        | BatchPhase::ApprovedForDelete
                        | BatchPhase::DeleteCompleted
                        | BatchPhase::SnapshotCompleted
                )
        })
        .map(|batch| batch.batch_id.clone())
        .collect()
}

fn ensure_no_failed_batches(state: &MigrationState) -> Result<(), CaravanError> {
    let failed_batches = failed_batches_requiring_review(state);
    if !failed_batches.is_empty() {
        return Err(CaravanError::InvalidArguments(format!(
            "one or more batches require operator review before continuing: {}",
            failed_batches.join(", ")
        )));
    }
    Ok(())
}

fn ensure_no_failed_verification_batches(state: &MigrationState) -> Result<(), CaravanError> {
    let failed_verification_batches = failed_verification_batches_requiring_review(state);
    if !failed_verification_batches.is_empty() {
        return Err(CaravanError::InvalidArguments(format!(
            "one or more batches failed verification and require operator review before continuing: {}",
            failed_verification_batches.join(", ")
        )));
    }
    Ok(())
}

fn ensure_no_operator_review_blocks(state: &MigrationState) -> Result<(), CaravanError> {
    ensure_no_failed_batches(state)?;
    ensure_no_failed_verification_batches(state)?;
    Ok(())
}

fn approve_and_delete_verified_batches(
    state: &mut MigrationState,
    source_root: &Path,
    interactive: bool,
    shutdown_flag: &ShutdownFlag,
    persist_state: &mut dyn FnMut(&MigrationState) -> Result<(), CaravanError>,
    delete_context: &str,
) -> Result<(), CaravanError> {
    // If a prior run already approved deletion, continue deletion without prompting.
    let already_approved = state.batches_approved_but_not_deleted();
    delete_batches_by_id(
        state,
        source_root,
        &already_approved,
        shutdown_flag,
        persist_state,
        delete_context,
    )?;

    let batches_needing_approval = state.batches_needing_approval();
    if !batches_needing_approval.is_empty() {
        let prompt_backend = prompt::InteractivePrompt;
        println!(
            "\n=== All {} batches have been verified successfully ===",
            batches_needing_approval.len()
        );

        let approved = prompt::request_approval_for_batches(
            Some(&prompt_backend),
            interactive,
            false,
            &batches_needing_approval,
        )?;

        if !approved {
            println!("Deletion not approved. Stopping.");
            return Ok(());
        }

        state.approve_batches(&batches_needing_approval);
        persist_state(state)?;

        delete_batches_by_id(
            state,
            source_root,
            &batches_needing_approval,
            shutdown_flag,
            persist_state,
            delete_context,
        )?;
    }

    Ok(())
}

fn delete_batches_by_id(
    state: &mut MigrationState,
    source_root: &Path,
    batch_ids: &[String],
    shutdown_flag: &ShutdownFlag,
    persist_state: &mut dyn FnMut(&MigrationState) -> Result<(), CaravanError>,
    delete_context: &str,
) -> Result<(), CaravanError> {
    if batch_ids.is_empty() {
        return Ok(());
    }

    println!(
        "\n=== Deleting source files for {} batch(es) ===",
        batch_ids.len()
    );
    for batch_id in batch_ids {
        check_shutdown(shutdown_flag)?;

        if let Some(batch_state) = state.batch(batch_id) {
            if batch_state.deleted {
                continue;
            }
            let batch = plan::load_batch_definition(
                source_root,
                batch_id,
                state.batch_size_bytes,
                state.max_files,
            )?;
            cleanup::cleanup_batch(&batch, source_root, state, delete_context)?;
            persist_state(state)?;
        }
    }

    Ok(())
}

fn verify_batch_for_resume(
    batch: &crate::models::batch::Batch,
    config: &TransferConfig,
    state: &mut MigrationState,
    state_path: &Path,
) -> Result<(), CaravanError> {
    println!("Verifying {}...", batch.id);
    let mut progress = crate::progress::TerminalProgress::new();
    let verification_report = verify::verify_batch_with_progress(
        batch,
        &config.source,
        &config.dest,
        config.verification.clone(),
        &mut progress,
    )?;

    let mut current_state = state.batch(&batch.id).cloned().ok_or_else(|| {
        CaravanError::InvalidArguments(format!(
            "batch {} disappeared from state during verification",
            batch.id
        ))
    })?;
    current_state.phase = BatchPhase::VerifyCompleted;
    current_state.verification_passed =
        verification_report.status == models::verification::VerificationStatus::Pass;
    state.upsert_batch(current_state.clone());
    state_store::persist_state(state_path, state)?;

    if !current_state.verification_passed {
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

    println!("✅ Verification passed!");
    Ok(())
}

fn copy_batch_for_resume(
    batch: &crate::models::batch::Batch,
    config: &TransferConfig,
    state: &mut MigrationState,
    state_path: &Path,
    copy_backend: &transfer::LocalFsCopyBackend,
) -> Result<(), CaravanError> {
    println!(
        "\n=== Processing {} ({} files, {}) ===",
        batch.id,
        batch.file_count,
        format::format_bytes(batch.total_bytes)
    );

    ensure_destination_capacity(&config.dest, batch.total_bytes)?;

    let mut current_state = state.batch(&batch.id).cloned().ok_or_else(|| {
        CaravanError::InvalidArguments(format!(
            "batch {} disappeared from state during copy",
            batch.id
        ))
    })?;
    current_state.phase = BatchPhase::CopyStarted;
    state.upsert_batch(current_state.clone());
    state_store::persist_state(state_path, state)?;

    let mut progress = crate::progress::TerminalProgress::new();
    transfer::transfer_batch_with_progress(
        batch,
        &config.source,
        &config.dest,
        copy_backend,
        &mut progress,
    )?;

    current_state.phase = BatchPhase::CopyCompleted;
    state.upsert_batch(current_state);
    state_store::persist_state(state_path, state)?;

    Ok(())
}

fn process_resume_batch(
    batch_id: &str,
    config: &TransferConfig,
    state: &mut MigrationState,
    state_path: &Path,
    copy_backend: &transfer::LocalFsCopyBackend,
) -> Result<(), CaravanError> {
    let batch_state = state
        .batch(batch_id)
        .ok_or_else(|| {
            CaravanError::InvalidArguments(format!(
                "Batch {} disappeared from state during resume iteration",
                batch_id
            ))
        })?
        .clone();

    if batch_state.deleted {
        println!("⏭️  Skipping {}: already completed", batch_state.batch_id);
        return Ok(());
    }

    let batch = plan::load_batch_definition(
        &config.source,
        &batch_state.batch_id,
        state.batch_size_bytes,
        state.max_files,
    )?;
    let recon = resume::reconcile_batch_destination(&batch, &config.dest);
    let step = resume::plan_resume_step(&batch_state, &recon, &batch);

    match step {
        resume::ResumeStepPlan::BatchFullyCompleted
        | resume::ResumeStepPlan::PostDeleteSnapshot => {
            println!("⏭️  Skipping {}: already completed", batch.id);
            Ok(())
        }
        resume::ResumeStepPlan::ConflictOperatorReview { reason } => {
            Err(CaravanError::InvalidArguments(format!(
                "batch {} requires operator review before continuing: {}",
                batch.id, reason
            )))
        }
        resume::ResumeStepPlan::BlockedFailedVerification => {
            Err(CaravanError::InvalidArguments(format!(
                "batch {} failed verification and requires operator review before continuing",
                batch.id
            )))
        }
        resume::ResumeStepPlan::DeleteSource | resume::ResumeStepPlan::PendingDeleteApproval => {
            println!(
                "✅ {} already verified, will continue to deletion phase",
                batch.id
            );
            Ok(())
        }
        resume::ResumeStepPlan::VerifyBatch => {
            verify_batch_for_resume(&batch, config, state, state_path)
        }
        resume::ResumeStepPlan::CopyBatch => {
            copy_batch_for_resume(&batch, config, state, state_path, copy_backend)?;
            verify_batch_for_resume(&batch, config, state, state_path)
        }
    }
}

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
                && existing_batch.phase == BatchPhase::VerifyCompleted
            {
                println!("Skipping {}: already verified", batch.id);
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

pub(super) fn execute_status(state_path: &Path) -> Result<(), CaravanError> {
    let state = state_store::load_state(state_path)?;

    println!("=== Caravan Status ===");
    println!("Mode: {}", state.mode);
    println!("Source: {}", state.source);
    println!("Destination: {}", state.destination);
    println!("Batches: {}", state.batches.len());

    for batch in &state.batches {
        println!(
            "  {} - {:?} (verified: {}, approved: {}, deleted: {})",
            batch.batch_id,
            batch.phase,
            batch.verification_passed,
            batch.approved_for_delete,
            batch.deleted
        );
    }

    if let Some(snapshot) = &state.last_successful_snapshot_name {
        println!("Last snapshot: {}", snapshot);
    }

    println!("\nJournal entries: {}", state.journal.len());
    for entry in state.journal.iter().rev().take(5) {
        println!(
            "  [{}] {} - {} ({})",
            entry.timestamp_unix_secs, entry.event, entry.batch_id, entry.context
        );
    }

    Ok(())
}

pub(super) fn execute_resume(state_path: &Path) -> Result<(), CaravanError> {
    // Initialize shutdown flag and install signal handlers
    let shutdown_flag = ShutdownFlag::new();
    install_signal_handlers(&shutdown_flag)?;

    let mut state = resume::resume_run(state_path)?;

    println!("=== Resuming from saved state ===");
    println!("Mode: {}", state.mode);
    println!("Source: {}", state.source);
    println!("Destination: {}", state.destination);
    println!("Total batches: {}", state.batches.len());

    // Count completed batches to calculate resume progress
    let completed_count = state.batches.iter().filter(|b| b.deleted).count();
    println!(
        "Completed batches: {} / {}",
        completed_count,
        state.batches.len()
    );

    ensure_no_operator_review_blocks(&state)?;

    // Reconstruct TransferConfig from saved state
    let config = TransferConfig {
        mode: match state.mode.as_str() {
            "staging" => Mode::Staging,
            "migrate" => Mode::Migrate,
            _ => {
                return Err(CaravanError::InvalidArguments(format!(
                    "Unknown mode in state: {}",
                    state.mode
                )));
            }
        },
        source: PathBuf::from(&state.source),
        dest: PathBuf::from(&state.destination),
        batch_size_bytes: state.batch_size_bytes,
        max_files: state.max_files,
        snapshot_every: state.snapshot_every,
        interactive: true,
        verification: state.verification_mode.clone(),
        log_level: "info".to_string(),
        skip_conflicts: false, // Default to false for resume
        copy_buffer_size: state.copy_buffer_size,
        buffered_copy_threshold: state.buffered_copy_threshold,
    };

    println!("Resuming transfer...\n");

    let copy_backend = transfer::LocalFsCopyBackend::with_config(
        config.copy_buffer_size,
        config.buffered_copy_threshold,
    );

    // Process batches directly from STATE, NOT rebuilding plan
    // Rebuilding plan would generate NEW DIFFERENT batch IDs that don't match existing state
    // which would cause resume to skip all actual work
    // We collect batch IDs first to avoid borrowing issues while mutating state during iteration
    let batch_ids: Vec<String> = state.batches.iter().map(|b| b.batch_id.clone()).collect();

    for batch_id in batch_ids {
        // Check for shutdown before starting batch
        check_shutdown(&shutdown_flag)?;
        process_resume_batch(&batch_id, &config, &mut state, state_path, &copy_backend)?;
    }

    // Check for shutdown before proceeding to deletion phase
    check_shutdown(&shutdown_flag)?;
    ensure_no_operator_review_blocks(&state)?;

    let mut persist_state =
        |current_state: &MigrationState| state_store::persist_state(state_path, current_state);
    approve_and_delete_verified_batches(
        &mut state,
        &config.source,
        config.interactive,
        &shutdown_flag,
        &mut persist_state,
        "resume",
    )?;

    // Count completed batches (including previously deleted ones)
    let completed_count = state.batches.iter().filter(|b| b.deleted).count();
    println!(
        "\n✅ Resume complete! {} batches processed, {} total completed",
        state.batches.len(),
        completed_count
    );

    Ok(())
}
