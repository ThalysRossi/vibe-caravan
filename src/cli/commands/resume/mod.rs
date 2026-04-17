use std::path::Path;

use crate::config::{OutputFormat, TransferConfig};
use crate::error::CaravanError;
use crate::migration_registry::MigrationStatus;
use crate::models::state::{MigrationPhase, MigrationState};
use crate::plan::PlanOptions;
use crate::signal::{install_signal_handlers, ShutdownFlag};
use crate::{plan, resume as resume_ops, snapshot, state_store};

use super::shared::{
    approve_and_delete_verified_batches, ensure_no_operator_review_blocks,
    ensure_no_operator_review_blocks_with_policy, print_resume_complete,
    print_resume_completed_batches, print_resume_state_details, print_resume_state_header,
    print_resuming_transfer, print_status_snapshot_policy, AppContext, OperatorReviewPolicy,
};

mod batch_flow;
mod config;
mod context;
mod step_handlers;

use batch_flow::run_resume_batches;
use config::transfer_config_from_state;
use context::ResumeContext;

fn ensure_resume_manifest_is_consistent(
    state: &mut MigrationState,
    config: &TransferConfig,
    state_path: &Path,
) -> Result<(), CaravanError> {
    let plan_opts = PlanOptions {
        batch_size_bytes: state.batch_size_bytes,
        max_files: state.max_files.map(|value| value as usize),
    };
    let snapshot = plan::build_plan(&config.source, &plan_opts)?;

    if state.planned_batches.is_empty() {
        eprintln!(
            "[WARNING] State file has no immutable batch manifest; seeding from current source plan."
        );
        state.planned_batches = plan::planned_batches_from_snapshot(&snapshot);
        state_store::persist_state(state_path, state)?;
        return Ok(());
    }

    plan::ensure_manifest_matches_snapshot(&state.planned_batches, &snapshot)
}

fn print_failed_batch_inspection_human(report: &resume_ops::FailedBatchInspectionReport) {
    println!("\n=== Failed Batch Inspection ===");
    if report.failed_batches.is_empty() {
        println!("No failed batches found in state.");
        return;
    }

    println!("Found {} failed batch(es).", report.failed_batch_count);
    for batch in &report.failed_batches {
        let recon = resume_ops::ReconciliationResult {
            all_destination_files_ready: batch.all_destination_files_ready,
            missing_in_destination: batch.missing_in_destination.clone(),
            size_mismatches: batch.size_mismatches.clone(),
        };
        println!(
            "{}: {}",
            batch.batch_id,
            resume_ops::reconciliation_summary(&recon)
        );
    }
}

fn print_failed_batch_inspection_json(
    report: &resume_ops::FailedBatchInspectionReport,
) -> Result<(), CaravanError> {
    let serialized = serde_json::to_string_pretty(report).map_err(|err| {
        CaravanError::Io(format!(
            "failed to serialize failed-batch inspection output: {err}"
        ))
    })?;
    println!("{serialized}");
    Ok(())
}

fn find_resume_migration_id(
    app_context: &AppContext,
    state: &MigrationState,
    state_path: &Path,
) -> Result<Option<u64>, CaravanError> {
    app_context.find_active_migration_id_for_state(
        &state.source,
        &state.destination,
        &state.mode,
        state_path,
    )
}

fn persist_resume_migration_status(
    app_context: &AppContext,
    migration_id: Option<u64>,
    status: MigrationStatus,
) -> Result<(), CaravanError> {
    let Some(migration_id) = migration_id else {
        return Ok(());
    };

    app_context.persist_migration_status(migration_id, status)
}

pub(in crate::cli) fn execute_resume(
    state_path: &Path,
    recover_failed: bool,
    inspect_failed: bool,
    output: OutputFormat,
) -> Result<(), CaravanError> {
    let shutdown_flag = ShutdownFlag::new();
    install_signal_handlers(&shutdown_flag)?;

    let mut state = resume_ops::load_state_for_resume(state_path)?;
    let config = transfer_config_from_state(&state, recover_failed)?;
    ensure_resume_manifest_is_consistent(&mut state, &config, state_path)?;

    if inspect_failed {
        let report = resume_ops::inspect_failed_batches(&state, &config.dest)?;
        match output {
            OutputFormat::Human => print_failed_batch_inspection_human(&report),
            OutputFormat::Json => print_failed_batch_inspection_json(&report)?,
        }
        return Ok(());
    }

    if output == OutputFormat::Json {
        return Err(CaravanError::InvalidArguments(
            "resume --output json is only supported with --inspect-failed".to_string(),
        ));
    }

    let app_context = AppContext::new();
    let migration_id = find_resume_migration_id(&app_context, &state, state_path)?;
    persist_resume_migration_status(&app_context, migration_id, MigrationStatus::Running)?;

    let resume_result = (|| -> Result<(), CaravanError> {
        let state_summary = resume_ops::summarize_resume_state(&state);
        print_resume_state_header();
        print_resume_state_details(
            state_summary.mode,
            state_summary.source,
            state_summary.destination,
            state_summary.total_batches,
        );

        print_resume_completed_batches(
            state_summary.completed_batches,
            state_summary.total_batches,
        );
        print_status_snapshot_policy(config.snapshot_every, config.snapshot_dir.as_deref());

        snapshot::validate_snapshot_configuration(
            config.mode.clone(),
            config.snapshot_every,
            &config.dest,
            config.snapshot_dir.as_deref(),
        )?;

        ensure_no_operator_review_blocks_with_policy(
            &state,
            OperatorReviewPolicy {
                // Failed batches are evaluated per-batch during resume planning so
                // operators can see destination reconciliation details.
                allow_failed_batches: true,
            },
        )?;

        print_resuming_transfer();

        let context = ResumeContext::new(&config, state_path, shutdown_flag);
        run_resume_batches(&mut state, &context)?;

        context.check_shutdown()?;
        ensure_no_operator_review_blocks(&state)?;

        state.migration_phase = MigrationPhase::AwaitingDeletion;
        context.persist_state(&state)?;
        persist_resume_migration_status(
            &app_context,
            migration_id,
            MigrationStatus::AwaitingDeletion,
        )?;

        let mut persist_state =
            |current_state: &MigrationState| context.persist_state(current_state);
        approve_and_delete_verified_batches(
            &mut state,
            &context.config.source,
            context.config.interactive,
            &context.shutdown_flag,
            &mut persist_state,
            "resume",
        )?;
        snapshot::process_pending_snapshots(
            context.config.mode.clone(),
            context.config.snapshot_every,
            &context.config.dest,
            context.config.snapshot_dir.as_deref(),
            &mut state,
            &context.snapshot_backend,
            &mut persist_state,
        )?;

        let execution_summary = resume_ops::summarize_resume_execution(&state);
        print_resume_complete(
            execution_summary.total_batches,
            execution_summary.completed_batches,
        );

        Ok(())
    })();

    match &resume_result {
        Ok(()) => {
            let has_pending_deletion = state.batches.iter().any(|batch| !batch.deleted);
            state.migration_phase = if has_pending_deletion {
                MigrationPhase::AwaitingDeletion
            } else {
                MigrationPhase::Completed
            };
            state_store::persist_state(state_path, &state)?;
            let final_status = if has_pending_deletion {
                MigrationStatus::AwaitingDeletion
            } else {
                MigrationStatus::Completed
            };
            persist_resume_migration_status(&app_context, migration_id, final_status)?;
        }
        Err(original_err) => {
            state.migration_phase = MigrationPhase::Failed;
            if let Err(state_err) = state_store::persist_state(state_path, &state) {
                eprintln!(
                    "[WARNING] resume failed and state phase could not be updated to failed: {}; original error: {}",
                    state_err, original_err
                );
            }
            if let Err(status_err) =
                persist_resume_migration_status(&app_context, migration_id, MigrationStatus::Failed)
            {
                eprintln!(
                    "[WARNING] resume failed and migration status could not be updated to failed: {}; original error: {}",
                    status_err, original_err
                );
            }
        }
    }

    resume_result
}
