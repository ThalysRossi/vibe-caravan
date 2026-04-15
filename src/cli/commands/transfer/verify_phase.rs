use std::path::Path;

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::format;
use crate::models::batch::Batch;
use crate::models::state::{BatchPhase, MigrationPhase, MigrationState};
use crate::signal::{check_shutdown, ShutdownFlag};

use super::super::shared::{persist_state_both_locations, verify_batch_with_state_updates};

fn verify_single_batch(
    batch: &Batch,
    config: &TransferConfig,
    state: &mut MigrationState,
    state_path: &Path,
    secondary_state_path: &Path,
) -> Result<(), CaravanError> {
    println!(
        "\n=== Verifying {} ({} files, {}) ===",
        batch.id,
        batch.file_count,
        format::format_bytes(batch.total_bytes)
    );

    let mut persist_state = |current_state: &MigrationState| {
        persist_state_both_locations(state_path, secondary_state_path, current_state)
    };
    verify_batch_with_state_updates(
        batch,
        &config.source,
        &config.dest,
        &config.verification,
        state,
        &mut persist_state,
        &|batch_id| {
            CaravanError::InvalidArguments(format!(
                "missing batch state for {} before verification",
                batch_id
            ))
        },
    )?;

    println!("Verification passed!");
    Ok(())
}

pub(super) fn run_verify_phase(
    config: &TransferConfig,
    plan: &crate::plan::PlanningSnapshot,
    state: &mut MigrationState,
    state_path: &Path,
    secondary_state_path: &Path,
    shutdown_flag: &ShutdownFlag,
) -> Result<u32, CaravanError> {
    state.migration_phase = MigrationPhase::Verifying;
    persist_state_both_locations(state_path, secondary_state_path, state)?;
    println!("\n=== Verifying all batches ===");

    let mut processed_batches = 0_u32;
    for batch in &plan.batches {
        check_shutdown(shutdown_flag)?;

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

        verify_single_batch(batch, config, state, state_path, secondary_state_path)?;
        processed_batches += 1;
    }

    Ok(processed_batches)
}
