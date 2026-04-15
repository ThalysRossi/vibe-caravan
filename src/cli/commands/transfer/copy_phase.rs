use std::path::Path;

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::state::{BatchPhase, MigrationPhase, MigrationState};
use crate::prompt::PromptBackend;
use crate::signal::{check_shutdown, ShutdownFlag};
use crate::{format, prompt, transfer};

use super::super::shared::{ensure_destination_capacity, persist_state_both_locations};
use super::setup::planned_batch_state;

fn copy_single_batch(
    batch: &Batch,
    config: &TransferConfig,
    state: &mut MigrationState,
    state_path: &Path,
    secondary_state_path: &Path,
    copy_backend: &transfer::LocalFsCopyBackend,
) -> Result<(), CaravanError> {
    println!(
        "\n=== Copying {} ({} files, {}) ===",
        batch.id,
        batch.file_count,
        format::format_bytes(batch.total_bytes)
    );

    let mut batch_state = state
        .batch(&batch.id)
        .cloned()
        .unwrap_or_else(|| planned_batch_state(&batch.id));

    ensure_destination_capacity(&config.dest, batch.total_bytes)?;

    let conflict_report = crate::conflict::detect_batch_conflicts(batch, &config.dest)?;
    if conflict_report.has_conflicts {
        let should_skip = if config.skip_conflicts || !config.interactive {
            true
        } else {
            let prompt_backend = prompt::InteractivePrompt;
            prompt_backend.confirm_conflict_skip(&batch.id, &conflict_report)?
        };

        if should_skip {
            println!(
                "⚠️  Skipping batch '{}' due to {} naming conflict(s)",
                batch.id, conflict_report.total_conflicts
            );

            batch_state.phase = BatchPhase::Failed;
            batch_state.verification_passed = false;
            state.upsert_batch(batch_state);
            persist_state_both_locations(state_path, secondary_state_path, state)?;
            return Ok(());
        }
    }

    batch_state.phase = BatchPhase::CopyStarted;
    state.upsert_batch(batch_state.clone());
    persist_state_both_locations(state_path, secondary_state_path, state)?;

    let mut progress = crate::progress::TerminalProgress::new();
    transfer::transfer_batch_with_progress(
        batch,
        &config.source,
        &config.dest,
        copy_backend,
        &mut progress,
    )?;

    batch_state.phase = BatchPhase::CopyCompleted;
    batch_state.verification_passed = false;
    state.upsert_batch(batch_state);
    persist_state_both_locations(state_path, secondary_state_path, state)?;

    Ok(())
}

pub(super) fn run_copy_phase(
    config: &TransferConfig,
    plan: &crate::plan::PlanningSnapshot,
    state: &mut MigrationState,
    state_path: &Path,
    secondary_state_path: &Path,
    shutdown_flag: &ShutdownFlag,
    copy_backend: &transfer::LocalFsCopyBackend,
) -> Result<u32, CaravanError> {
    state.migration_phase = MigrationPhase::Copying;
    persist_state_both_locations(state_path, secondary_state_path, state)?;
    println!("\n=== Copying all batches ===");

    let mut processed_batches = 0_u32;
    for batch in &plan.batches {
        check_shutdown(shutdown_flag)?;

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

        copy_single_batch(
            batch,
            config,
            state,
            state_path,
            secondary_state_path,
            copy_backend,
        )?;
    }

    Ok(processed_batches)
}
