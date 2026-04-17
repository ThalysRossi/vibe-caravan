use std::path::Path;

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::state::{BatchState, MigrationState};
use crate::signal::{check_shutdown, ShutdownFlag};
use crate::{resume as resume_ops, transfer};

use super::super::shared::print_resume_skip_already_completed;
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
    config: &TransferConfig,
) -> resume_ops::ResumeStepPlan {
    let recon = resume_ops::reconcile_batch_destination(batch, &config.dest);
    resume_ops::plan_resume_step_with_recovery(batch_state, &recon, batch, config.recover_failed)
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
    let step = plan_next_step(&batch_state, &batch, config);
    execute_resume_step(step, &batch, config, state, state_path, copy_backend)
}

pub(super) fn run_resume_batches(
    state: &mut MigrationState,
    config: &TransferConfig,
    state_path: &Path,
    shutdown_flag: &ShutdownFlag,
    copy_backend: &transfer::LocalFsCopyBackend,
) -> Result<(), CaravanError> {
    let batch_ids: Vec<String> = state.batches.iter().map(|b| b.batch_id.clone()).collect();

    for batch_id in batch_ids {
        check_shutdown(shutdown_flag)?;
        process_resume_batch(&batch_id, config, state, state_path, copy_backend)?;
    }

    Ok(())
}
