use std::path::PathBuf;

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::plan::PlanOptions;
use crate::signal::{install_signal_handlers, ShutdownFlag};
use crate::{migration_registry, plan, preflight, snapshot, transfer};

use super::shared::{
    ensure_no_operator_review_blocks_with_policy, persist_state_both_locations,
    print_migration_complete, print_plan_summary, print_preflight_warnings,
    print_state_save_locations, print_status_snapshot_policy, OperatorReviewPolicy,
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
    set_migration_status, warn_copy_backend_config,
};
use verify_phase::run_verify_phase;

fn state_has_manifest(state: &MigrationState) -> bool {
    !state.planned_batches.is_empty()
}

pub(super) fn execute_transfer(config: TransferConfig) -> Result<(), CaravanError> {
    let shutdown_flag = ShutdownFlag::new();
    install_signal_handlers(&shutdown_flag)?;

    let state_path = migration_registry::state_file_in_source(&config.source, &config.dest);
    let secondary_state_path = PathBuf::from(".caravan/state.json");

    let source_str = config.source.to_string_lossy().to_string();
    let dest_str = config.dest.to_string_lossy().to_string();
    let mode = mode_name(&config.mode);
    let state_filename = migration_registry::generate_state_filename(&source_str, &dest_str);

    let migration_id = register_migration(&source_str, &dest_str, mode, &state_filename)?;

    let transfer_result = (|| -> Result<(), CaravanError> {
        let mut state = load_or_create_state(
            &config,
            &state_path,
            &secondary_state_path,
            mode,
            &source_str,
            &dest_str,
        )?;
        apply_transfer_config(&mut state, &config);

        ensure_no_operator_review_blocks_with_policy(
            &state,
            OperatorReviewPolicy {
                allow_failed_batches: config.recover_failed,
            },
        )?;

        print_state_save_locations(&state_path, &secondary_state_path);
        print_status_snapshot_policy(config.snapshot_every, config.snapshot_dir.as_deref());

        let plan_opts = PlanOptions {
            batch_size_bytes: config.batch_size_bytes,
            max_files: config.max_files.map(|v| v as usize),
        };
        let plan = plan::build_plan(&config.source, &plan_opts)?;

        if state_has_manifest(&state) {
            plan::ensure_manifest_matches_snapshot(&state.planned_batches, &plan)?;
        } else {
            eprintln!(
                "[WARNING] State file has no immutable batch manifest; seeding from current source plan."
            );
            state.planned_batches = plan::planned_batches_from_snapshot(&plan);
        }

        print_plan_summary(
            plan.batches.len(),
            plan.source_file_count,
            plan.source_total_bytes,
        );
        let preflight_report = preflight::analyze_transfer_preflight(&config, &plan)?;
        print_preflight_warnings(&preflight_report.warnings);
        preflight::enforce_transfer_preflight_policy(&config, &preflight_report)?;
        snapshot::validate_snapshot_configuration(
            config.mode.clone(),
            config.snapshot_every,
            &config.dest,
            config.snapshot_dir.as_deref(),
        )?;

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
        set_migration_status(migration_id, migration_registry::MigrationStatus::Verifying)?;
        processed_batches += run_verify_phase(
            &config,
            &plan,
            &mut state,
            &state_path,
            &secondary_state_path,
            &shutdown_flag,
        )?;
        set_migration_status(
            migration_id,
            migration_registry::MigrationStatus::AwaitingDeletion,
        )?;
        run_delete_phase(
            &config,
            &mut state,
            &state_path,
            &secondary_state_path,
            &shutdown_flag,
        )?;
        let snapshot_backend = snapshot::SystemSnapshotBackend;
        let mut persist_state = |current_state: &MigrationState| {
            persist_state_both_locations(&state_path, &secondary_state_path, current_state)
        };
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
        print_migration_complete(processed_batches, completed_count);
        Ok(())
    })();

    match &transfer_result {
        Ok(()) => {
            set_migration_status(migration_id, migration_registry::MigrationStatus::Completed)?;
        }
        Err(original_err) => {
            if let Err(status_err) =
                set_migration_status(migration_id, migration_registry::MigrationStatus::Failed)
            {
                eprintln!(
                    "[WARNING] transfer failed and migration status could not be updated to failed: {}; original error: {}",
                    status_err, original_err
                );
            }
        }
    }

    transfer_result
}
