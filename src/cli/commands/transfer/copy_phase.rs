use std::path::Path;

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::models::state::{BatchPhase, MigrationPhase, MigrationState};
use crate::signal::{check_shutdown, ShutdownFlag};
use crate::transfer;

use super::super::shared::{
    persist_state_both_locations, print_phase_banner, print_skip_already_completed,
    print_skip_copy_already_completed,
};
use super::batch_handlers::copy_single_batch;

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
    print_phase_banner("Copying all batches");

    let mut processed_batches = 0_u32;
    for batch in &plan.batches {
        check_shutdown(shutdown_flag)?;

        if let Some(existing_batch) = state.batch(&batch.id) {
            if existing_batch.deleted {
                print_skip_already_completed(&batch.id);
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
                print_skip_copy_already_completed(&batch.id, existing_batch.phase);
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
