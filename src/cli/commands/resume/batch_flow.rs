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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::state::{BatchPhase, PlannedBatch, PlannedFile};
    use std::path::PathBuf;

    #[test]
    fn load_batch_for_resume_errors_when_manifest_is_missing() {
        let state = MigrationState::new("staging", "/src", "/dst");
        let batch_state = BatchState {
            batch_id: "batch-000001".to_string(),
            phase: BatchPhase::Planned,
            verification_passed: false,
            approved_for_delete: false,
            deleted: false,
        };

        let err = load_batch_for_resume(&batch_state, &state)
            .expect_err("missing planned batch must be treated as corrupt state");
        assert!(err.to_string().contains("missing immutable batch manifest"));
    }

    #[test]
    fn load_batch_for_resume_materializes_batch_from_manifest() {
        let mut state = MigrationState::new("staging", "/src", "/dst");
        state.upsert_planned_batch(PlannedBatch {
            batch_id: "batch-000001".to_string(),
            file_count: 1,
            total_bytes: 7,
            files: vec![PlannedFile {
                relative_path: PathBuf::from("a.txt"),
                size_bytes: 7,
            }],
        });
        let batch_state = BatchState {
            batch_id: "batch-000001".to_string(),
            phase: BatchPhase::Planned,
            verification_passed: false,
            approved_for_delete: false,
            deleted: false,
        };

        let batch = load_batch_for_resume(&batch_state, &state).expect("batch should load");
        assert_eq!(batch.id, "batch-000001");
        assert_eq!(batch.file_count, 1);
        assert_eq!(batch.total_bytes, 7);
        assert_eq!(batch.files[0].relative_path, PathBuf::from("a.txt"));
    }
}
