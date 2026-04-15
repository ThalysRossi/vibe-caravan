use std::path::Path;

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::models;
use crate::models::batch::Batch;
use crate::models::state::{BatchPhase, MigrationPhase, MigrationState};
use crate::signal::{check_shutdown, ShutdownFlag};
use crate::{format, verify};

use super::super::shared::persist_state_both_locations;

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

    let mut batch_state = state.batch(&batch.id).cloned().ok_or_else(|| {
        CaravanError::InvalidArguments(format!(
            "missing batch state for {} before verification",
            batch.id
        ))
    })?;

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
    persist_state_both_locations(state_path, secondary_state_path, state)?;

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
