use std::path::Path;

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::state::{BatchPhase, MigrationPhase, MigrationState};
use crate::signal::{check_shutdown, ShutdownFlag};

use super::super::shared::{
    persist_state_both_locations, print_phase_banner, print_skip_verification_already_completed,
    print_skip_verification_requires_operator_review, print_verification_passed,
    print_verify_batch_banner, verify_batch_with_state_updates,
};

fn verify_single_batch(
    batch: &Batch,
    config: &TransferConfig,
    state: &mut MigrationState,
    state_path: &Path,
    secondary_state_path: &Path,
) -> Result<(), CaravanError> {
    print_verify_batch_banner(batch);

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

    print_verification_passed();
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
    print_phase_banner("Verifying all batches");

    let mut processed_batches = 0_u32;
    for batch in &plan.batches {
        check_shutdown(shutdown_flag)?;

        if let Some(existing_batch) = state.batch(&batch.id) {
            if existing_batch.deleted {
                continue;
            }
            if existing_batch.phase == BatchPhase::Failed {
                print_skip_verification_requires_operator_review(&batch.id);
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
                print_skip_verification_already_completed(&batch.id, existing_batch.phase);
                processed_batches += 1;
                continue;
            }
        }

        verify_single_batch(batch, config, state, state_path, secondary_state_path)?;
        processed_batches += 1;
    }

    Ok(processed_batches)
}
