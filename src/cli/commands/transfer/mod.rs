use std::path::PathBuf;

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::plan::PlanOptions;
use crate::signal::{install_signal_handlers, ShutdownFlag};
use crate::{migration_registry, plan, preflight, transfer};

use super::shared::{
    ensure_no_operator_review_blocks_with_policy, persist_state_both_locations,
    print_migration_complete, print_plan_summary, print_preflight_warnings,
    print_state_save_locations, OperatorReviewPolicy,
};

mod batch_handlers;
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

    ensure_no_operator_review_blocks_with_policy(
        &state,
        OperatorReviewPolicy {
            allow_failed_batches: config.recover_failed,
        },
    )?;

    print_state_save_locations(&state_path, &secondary_state_path);

    let plan_opts = PlanOptions {
        batch_size_bytes: config.batch_size_bytes,
        max_files: config.max_files.map(|v| v as usize),
    };
    let plan = plan::build_plan(&config.source, &plan_opts)?;

    print_plan_summary(
        plan.batches.len(),
        plan.source_file_count,
        plan.source_total_bytes,
    );
    let preflight_report = preflight::analyze_transfer_preflight(&config, &plan)?;
    print_preflight_warnings(&preflight_report.warnings);
    preflight::enforce_transfer_preflight_policy(&config, &preflight_report)?;

    seed_state_batches(&mut state, &plan);
    persist_state_both_locations(&state_path, &secondary_state_path, &state)?;

    let copy_backend = transfer::LocalFsCopyBackend::with_transfer_config(&config);
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
    print_migration_complete(processed_batches, completed_count);
    Ok(())
}
