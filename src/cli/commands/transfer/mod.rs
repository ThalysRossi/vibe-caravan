use std::path::PathBuf;

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::plan::PlanOptions;
use crate::signal::{install_signal_handlers, ShutdownFlag};
use crate::{format, migration_registry, plan, transfer};

use super::shared::{ensure_no_operator_review_blocks, persist_state_both_locations};

mod copy_phase;
mod delete_phase;
mod setup;
mod verify_phase;

use copy_phase::run_copy_phase;
use delete_phase::run_delete_phase;
use setup::{
    apply_transfer_config, load_or_create_state, mode_name, register_migration, seed_state_batches,
    warn_copy_backend_config,
};
use verify_phase::run_verify_phase;

pub(super) fn execute_transfer(config: TransferConfig) -> Result<(), CaravanError> {
    let shutdown_flag = ShutdownFlag::new();
    install_signal_handlers(&shutdown_flag)?;

    let state_path = migration_registry::state_file_in_source(&config.source, &config.dest);
    let secondary_state_path = PathBuf::from(".caravan/state.json");

    let source_str = config.source.to_string_lossy().to_string();
    let dest_str = config.dest.to_string_lossy().to_string();
    let mode = mode_name(&config.mode);
    let state_filename = migration_registry::generate_state_filename(&source_str, &dest_str);

    register_migration(&source_str, &dest_str, mode, &state_filename)?;

    let mut state = load_or_create_state(&config, &state_path, mode, &source_str, &dest_str)?;
    apply_transfer_config(&mut state, &config);

    ensure_no_operator_review_blocks(&state)?;

    println!(
        "State will be saved to: {} (primary) and {} (backward compatibility)",
        state_path.display(),
        secondary_state_path.display()
    );

    let plan_opts = PlanOptions {
        batch_size_bytes: config.batch_size_bytes,
        max_files: config.max_files.map(|v| v as usize),
    };
    let plan = plan::build_plan(&config.source, &plan_opts)?;

    println!(
        "Planned {} batches for {} files ({} total)",
        plan.batches.len(),
        plan.source_file_count,
        format::format_bytes(plan.source_total_bytes)
    );

    seed_state_batches(&mut state, &plan);
    persist_state_both_locations(&state_path, &secondary_state_path, &state)?;

    let copy_backend = transfer::LocalFsCopyBackend::with_config(
        config.copy_buffer_size,
        config.buffered_copy_threshold,
    );
    warn_copy_backend_config(&config);

    let mut processed_batches = 0_u32;
    processed_batches += run_copy_phase(
        &config,
        &plan,
        &mut state,
        &state_path,
        &secondary_state_path,
        &shutdown_flag,
        &copy_backend,
    )?;
    processed_batches += run_verify_phase(
        &config,
        &plan,
        &mut state,
        &state_path,
        &secondary_state_path,
        &shutdown_flag,
    )?;
    run_delete_phase(
        &config,
        &mut state,
        &state_path,
        &secondary_state_path,
        &shutdown_flag,
    )?;

    let completed_count = state.batches.iter().filter(|b| b.deleted).count();
    println!(
        "\n=== Migration complete! {} batches processed, {} total completed ===",
        processed_batches, completed_count
    );
    Ok(())
}
