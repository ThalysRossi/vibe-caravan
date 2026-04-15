use std::path::Path;

use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::signal::{check_shutdown, install_signal_handlers, ShutdownFlag};
use crate::{resume as resume_ops, state_store, transfer};

use super::shared::{approve_and_delete_verified_batches, ensure_no_operator_review_blocks};

mod batch_flow;
mod config;

use batch_flow::run_resume_batches;
use config::transfer_config_from_state;

pub(super) fn execute_resume(state_path: &Path) -> Result<(), CaravanError> {
    let shutdown_flag = ShutdownFlag::new();
    install_signal_handlers(&shutdown_flag)?;

    let mut state = resume_ops::resume_run(state_path)?;

    println!("=== Resuming from saved state ===");
    println!("Mode: {}", state.mode);
    println!("Source: {}", state.source);
    println!("Destination: {}", state.destination);
    println!("Total batches: {}", state.batches.len());

    let completed_count = state.batches.iter().filter(|b| b.deleted).count();
    println!(
        "Completed batches: {} / {}",
        completed_count,
        state.batches.len()
    );

    ensure_no_operator_review_blocks(&state)?;

    let config = transfer_config_from_state(&state)?;

    println!("Resuming transfer...\n");

    let copy_backend = transfer::LocalFsCopyBackend::with_config(
        config.copy_buffer_size,
        config.buffered_copy_threshold,
    );

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

    let completed_count = state.batches.iter().filter(|b| b.deleted).count();
    println!(
        "\n✅ Resume complete! {} batches processed, {} total completed",
        state.batches.len(),
        completed_count
    );

    Ok(())
}
