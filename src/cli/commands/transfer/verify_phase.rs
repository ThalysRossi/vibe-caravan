use crate::error::CaravanError;
use crate::models::state::{BatchPhase, MigrationPhase, MigrationState};
use crate::signal::check_shutdown;

use super::super::shared::{
    print_phase_banner, print_skip_verification_already_completed,
    print_skip_verification_requires_operator_review,
};
use super::batch_handlers::verify_single_batch;
use super::context::TransferContext;

pub(super) fn run_verify_phase(
    context: &TransferContext<'_>,
    plan: &crate::plan::PlanningSnapshot,
    state: &mut MigrationState,
) -> Result<u32, CaravanError> {
    state.migration_phase = MigrationPhase::Verifying;
    context.persist_state(state)?;
    print_phase_banner("Verifying all batches");

    let mut processed_batches = 0_u32;
    for batch in &plan.batches {
        check_shutdown(&context.shutdown_flag)?;

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

        verify_single_batch(batch, context, state)?;
        processed_batches += 1;
    }

    Ok(processed_batches)
}
