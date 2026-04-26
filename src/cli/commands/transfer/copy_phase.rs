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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConflictPolicy, CopyStrategy, Mode, TransferConfig};
    use crate::models::batch::Batch;
    use crate::models::state::BatchState;
    use crate::signal::ShutdownFlag;
    use crate::transfer::LocalFsCopyBackend;

    fn sample_config(source: &std::path::Path, dest: &std::path::Path) -> TransferConfig {
        TransferConfig {
            mode: Mode::Staging,
            source: source.to_path_buf(),
            dest: dest.to_path_buf(),
            batch_size_bytes: 1024,
            max_files: None,
            snapshot_every: None,
            snapshot_dir: None,
            interactive: false,
            log_level: "info".to_string(),
            skip_conflicts: false,
            conflict_policy: ConflictPolicy::SkipFile,
            recover_failed: false,
            allow_unsafe_filesystems: false,
            copy_strategy: CopyStrategy::Auto,
        }
    }

    fn sample_plan(batch_id: &str) -> crate::plan::PlanningSnapshot {
        crate::plan::PlanningSnapshot {
            source_file_count: 0,
            source_total_bytes: 0,
            batches: vec![Batch {
                id: batch_id.to_string(),
                files: Vec::new(),
                total_bytes: 0,
                file_count: 0,
            }],
        }
    }

    #[test]
    fn run_copy_phase_counts_deleted_batches_as_processed() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let source = tmp.path().join("source");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&source).expect("create source");
        std::fs::create_dir_all(&dest).expect("create dest");

        let config = sample_config(&source, &dest);
        let context = TransferContext {
            config: &config,
            state_path: tmp.path().join("state.json"),
            secondary_state_path: tmp.path().join("state_compat.json"),
            shutdown_flag: ShutdownFlag::new(),
            copy_backend: LocalFsCopyBackend::with_transfer_config(&config),
            snapshot_backend: crate::snapshot::SystemSnapshotBackend,
        };

        let mut state = MigrationState::new("staging", "/src", "/dst");
        state.upsert_batch(BatchState {
            batch_id: "batch-000001".to_string(),
            phase: BatchPhase::DeleteCompleted,
            verification_passed: true,
            approved_for_delete: true,
            deleted: true,
        });

        let processed = run_copy_phase(&context, &sample_plan("batch-000001"), &mut state)
            .expect("copy phase should succeed");

        assert_eq!(processed, 1);
        assert_eq!(state.migration_phase, MigrationPhase::Copying);
        assert!(
            std::path::Path::new(&context.state_path).exists(),
            "state should be persisted before processing batches"
        );
    }
}
