use std::path::Path;

use serde::Serialize;

use crate::config::{OutputFormat, TransferConfig};
use crate::error::CaravanError;
use crate::models::state::{BatchPhase, MigrationState};
use crate::plan::PlanOptions;
use crate::signal::{install_signal_handlers, ShutdownFlag};
use crate::{plan, resume as resume_ops, snapshot, state_store};

use super::shared::{
    approve_and_delete_verified_batches, ensure_no_operator_review_blocks,
    ensure_no_operator_review_blocks_with_policy, print_resume_complete,
    print_resume_completed_batches, print_resume_state_details, print_resume_state_header,
    print_resuming_transfer, print_status_snapshot_policy, OperatorReviewPolicy,
};

mod batch_flow;
mod config;
mod context;
mod step_handlers;

use batch_flow::run_resume_batches;
use config::transfer_config_from_state;
use context::ResumeContext;

#[derive(Debug, Serialize)]
struct FailedBatchInspection {
    batch_id: String,
    all_destination_files_ready: bool,
    missing_in_destination: Vec<String>,
    size_mismatches: Vec<String>,
}

#[derive(Debug, Serialize)]
struct FailedBatchInspectionReport {
    failed_batch_count: usize,
    failed_batches: Vec<FailedBatchInspection>,
}

fn build_failed_batch_inspection_report(
    state: &MigrationState,
    config: &TransferConfig,
) -> Result<FailedBatchInspectionReport, CaravanError> {
    let failed_batch_ids: Vec<String> = state
        .batches
        .iter()
        .filter(|batch| batch.phase == BatchPhase::Failed && !batch.deleted)
        .map(|batch| batch.batch_id.clone())
        .collect();

    let mut failed_batches = Vec::with_capacity(failed_batch_ids.len());
    for batch_id in failed_batch_ids {
        let batch = state.materialize_planned_batch(&batch_id).ok_or_else(|| {
            CaravanError::StateCorrupt(format!(
                "missing immutable batch manifest for {}; cannot inspect failed batch safely",
                batch_id
            ))
        })?;
        let recon = resume_ops::reconcile_batch_destination(&batch, &config.dest);
        failed_batches.push(FailedBatchInspection {
            batch_id,
            all_destination_files_ready: recon.all_destination_files_ready,
            missing_in_destination: recon.missing_in_destination,
            size_mismatches: recon.size_mismatches,
        });
    }

    Ok(FailedBatchInspectionReport {
        failed_batch_count: failed_batches.len(),
        failed_batches,
    })
}

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

fn print_failed_batch_inspection_human(report: &FailedBatchInspectionReport) {
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
    report: &FailedBatchInspectionReport,
) -> Result<(), CaravanError> {
    let serialized = serde_json::to_string_pretty(report).map_err(|err| {
        CaravanError::Io(format!(
            "failed to serialize failed-batch inspection output: {err}"
        ))
    })?;
    println!("{serialized}");
    Ok(())
}

pub(super) fn execute_resume(
    state_path: &Path,
    recover_failed: bool,
    inspect_failed: bool,
    output: OutputFormat,
) -> Result<(), CaravanError> {
    let shutdown_flag = ShutdownFlag::new();
    install_signal_handlers(&shutdown_flag)?;

    let mut state = resume_ops::resume_run(state_path)?;
    let config = transfer_config_from_state(&state, recover_failed)?;
    ensure_resume_manifest_is_consistent(&mut state, &config, state_path)?;

    if inspect_failed {
        let report = build_failed_batch_inspection_report(&state, &config)?;
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

    print_resume_state_header();
    print_resume_state_details(
        &state.mode,
        &state.source,
        &state.destination,
        state.batches.len(),
    );

    let completed_count = state.batches.iter().filter(|b| b.deleted).count();
    print_resume_completed_batches(completed_count, state.batches.len());
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

    let mut persist_state = |current_state: &MigrationState| context.persist_state(current_state);
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

    let completed_count = state.batches.iter().filter(|b| b.deleted).count();
    print_resume_complete(state.batches.len(), completed_count);

    Ok(())
}
