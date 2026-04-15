use std::path::Path;

use crate::error::CaravanError;
use crate::models::state::{BatchPhase, MigrationState};
use crate::signal::{check_shutdown, install_signal_handlers, ShutdownFlag};
use crate::{plan, resume as resume_ops, snapshot, state_store, transfer};

use super::shared::{
    approve_and_delete_verified_batches, ensure_no_operator_review_blocks,
    ensure_no_operator_review_blocks_with_policy, print_resume_complete,
    print_resume_completed_batches, print_resume_state_details, print_resume_state_header,
    print_resuming_transfer, OperatorReviewPolicy,
};

mod batch_flow;
mod config;
mod step_handlers;

use batch_flow::run_resume_batches;
use config::transfer_config_from_state;

fn inspect_failed_batches(
    state: &MigrationState,
    config: &crate::config::TransferConfig,
) -> Result<(), CaravanError> {
    let failed_batch_ids: Vec<String> = state
        .batches
        .iter()
        .filter(|batch| batch.phase == BatchPhase::Failed && !batch.deleted)
        .map(|batch| batch.batch_id.clone())
        .collect();

    println!("\n=== Failed Batch Inspection ===");
    if failed_batch_ids.is_empty() {
        println!("No failed batches found in state.");
        return Ok(());
    }

    println!("Found {} failed batch(es).", failed_batch_ids.len());
    for batch_id in failed_batch_ids {
        let batch = plan::load_batch_definition(
            &config.source,
            &batch_id,
            state.batch_size_bytes,
            state.max_files,
        )?;
        let recon = resume_ops::reconcile_batch_destination(&batch, &config.dest);
        println!(
            "{}: {}",
            batch_id,
            resume_ops::reconciliation_summary(&recon)
        );
    }

    Ok(())
}

pub(super) fn execute_resume(
    state_path: &Path,
    recover_failed: bool,
    inspect_failed: bool,
) -> Result<(), CaravanError> {
    let shutdown_flag = ShutdownFlag::new();
    install_signal_handlers(&shutdown_flag)?;

    let mut state = resume_ops::resume_run(state_path)?;

    print_resume_state_header();
    print_resume_state_details(
        &state.mode,
        &state.source,
        &state.destination,
        state.batches.len(),
    );

    let completed_count = state.batches.iter().filter(|b| b.deleted).count();
    print_resume_completed_batches(completed_count, state.batches.len());

    let config = transfer_config_from_state(&state, recover_failed)?;
    if inspect_failed {
        inspect_failed_batches(&state, &config)?;
        return Ok(());
    }

    ensure_no_operator_review_blocks_with_policy(
        &state,
        OperatorReviewPolicy {
            // Failed batches are evaluated per-batch during resume planning so
            // operators can see destination reconciliation details.
            allow_failed_batches: true,
        },
    )?;

    print_resuming_transfer();

    let copy_backend = transfer::LocalFsCopyBackend::with_transfer_config(&config);

    run_resume_batches(
        &mut state,
        &config,
        state_path,
        &shutdown_flag,
        &copy_backend,
    )?;

    check_shutdown(&shutdown_flag)?;
    ensure_no_operator_review_blocks(&state)?;

    let mut persist_state =
        |current_state: &MigrationState| state_store::persist_state(state_path, current_state);
    approve_and_delete_verified_batches(
        &mut state,
        &config.source,
        config.interactive,
        &shutdown_flag,
        &mut persist_state,
        "resume",
    )?;
    let snapshot_backend = snapshot::SystemSnapshotBackend;
    snapshot::process_pending_snapshots(
        config.mode.clone(),
        config.snapshot_every,
        &config.dest,
        config.snapshot_dir.as_deref(),
        &mut state,
        &snapshot_backend,
        &mut persist_state,
    )?;

    let completed_count = state.batches.iter().filter(|b| b.deleted).count();
    print_resume_complete(state.batches.len(), completed_count);

    Ok(())
}
