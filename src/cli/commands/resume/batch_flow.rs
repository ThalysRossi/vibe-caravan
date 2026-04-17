use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::state::{BatchState, MigrationState};
use crate::resume as resume_ops;

use super::super::shared::print_resume_skip_already_completed;
use super::context::ResumeContext;
use super::step_handlers::execute_resume_step;

fn load_batch_for_resume(
    batch_state: &BatchState,
    state: &MigrationState,
) -> Result<Batch, CaravanError> {
    state
        .materialize_planned_batch(&batch_state.batch_id)
        .ok_or_else(|| {
            CaravanError::StateCorrupt(format!(
                "missing immutable batch manifest for {}; cannot continue resume",
                batch_state.batch_id
            ))
        })
}

fn plan_next_step(
    batch_state: &BatchState,
    batch: &Batch,
    context: &ResumeContext<'_>,
) -> resume_ops::ResumeStepPlan {
    let recon = resume_ops::reconcile_batch_destination(batch, &context.config.dest);
    resume_ops::plan_resume_step_with_recovery(
        batch_state,
        &recon,
        batch,
        context.config.recover_failed,
    )
}

fn process_resume_batch(
    batch_id: &str,
    context: &ResumeContext<'_>,
    state: &mut MigrationState,
) -> Result<(), CaravanError> {
    let batch_state = state
        .batch(batch_id)
        .ok_or_else(|| {
            CaravanError::StateCorrupt(format!(
                "Batch {} disappeared from state during resume iteration",
                batch_id
            ))
        })?
        .clone();

    if batch_state.deleted {
        print_resume_skip_already_completed(&batch_state.batch_id);
        return Ok(());
    }

    let batch = load_batch_for_resume(&batch_state, state)?;
    let step = plan_next_step(&batch_state, &batch, context);
    execute_resume_step(step, &batch, context, state)
}

pub(super) fn run_resume_batches(
    state: &mut MigrationState,
    context: &ResumeContext<'_>,
) -> Result<(), CaravanError> {
    let batch_ids: Vec<String> = state.batches.iter().map(|b| b.batch_id.clone()).collect();

    for batch_id in batch_ids {
        context.check_shutdown()?;
        process_resume_batch(&batch_id, context, state)?;
    }

    Ok(())
}
