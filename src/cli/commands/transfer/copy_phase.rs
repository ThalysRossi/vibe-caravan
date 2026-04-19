use crate::error::CaravanError;
use crate::models::state::{BatchPhase, MigrationPhase, MigrationState};
use crate::signal::check_shutdown;

use super::super::shared::{
    print_phase_banner, print_skip_already_completed, print_skip_copy_already_completed,
};
use super::batch_handlers::copy_single_batch;
use super::context::TransferContext;

pub(super) fn run_copy_phase(
    context: &TransferContext<'_>,
    plan: &crate::plan::PlanningSnapshot,
    state: &mut MigrationState,
) -> Result<u32, CaravanError> {
    state.migration_phase = MigrationPhase::Copying;
    context.persist_state(state)?;
    print_phase_banner("Copying all batches");

    let mut processed_batches = 0_u32;
    for batch in &plan.batches {
        check_shutdown(&context.shutdown_flag)?;

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
                let reconciliation =
                    crate::resume::reconcile_batch_destination(batch, &context.config.dest);
                if reconciliation.all_destination_files_ready {
                    print_skip_copy_already_completed(&batch.id, existing_batch.phase);
                    continue;
                }
                eprintln!(
                    "[WARNING] Batch '{}' marked {:?} but destination is incomplete (missing={}, mismatched={}); re-entering copy phase with current conflict policy.",
                    batch.id,
                    existing_batch.phase,
                    reconciliation.missing_in_destination.len(),
                    reconciliation.size_mismatches.len()
                );
            }
        }

        copy_single_batch(batch, context, state)?;
    }

    Ok(processed_batches)
}
