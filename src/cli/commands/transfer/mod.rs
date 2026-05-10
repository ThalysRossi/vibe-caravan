use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::models::state::{MigrationPhase, MigrationState};
use crate::plan::PlanOptions;
use crate::transfer as transfer_ops;
use crate::{migration_registry, plan, preflight, progress, scan, snapshot, source_completion};

use super::shared::{
    AppContext, OperatorReviewPolicy, ensure_no_operator_review_blocks_with_policy,
    print_migration_complete, print_plan_summary, print_preflight_warnings,
    print_state_save_locations, print_status_snapshot_policy,
};

mod batch_handlers;
mod context;
mod copy_phase;
mod delete_phase;
mod setup;
mod verify_phase;

use context::TransferContext;
use copy_phase::run_copy_phase;
use delete_phase::run_delete_phase;
use setup::{
    apply_transfer_config, load_or_create_state, mode_name, register_migration, seed_state_batches,
    warn_copy_backend_config,
};
use verify_phase::run_verify_phase;

fn state_has_manifest(state: &MigrationState) -> bool {
    !state.planned_batches.is_empty() || !state.skipped_completed_files.is_empty()
}

fn state_is_empty_for_completed_file_filtering(state: &MigrationState) -> bool {
    state.planned_batches.is_empty()
        && state.batches.is_empty()
        && state.skipped_completed_files.is_empty()
}

fn build_plan_for_transfer_state(
    config: &TransferConfig,
    state: &mut MigrationState,
    mode: &str,
    options: &PlanOptions,
) -> Result<plan::PlanningSnapshot, CaravanError> {
    if state_is_empty_for_completed_file_filtering(state) {
        let entries = scan::scan_source(&config.source)?;
        let mut hashing_progress = progress::TerminalProgress::new();
        let hashed_entries = source_completion::hash_source_entries_with_progress(
            &config.source,
            &entries,
            &mut hashing_progress,
        )?;
        source_completion::backfill_ledger_from_existing_states(
            &config.source,
            mode,
            &hashed_entries,
        )?;
        let ledger = source_completion::load_ledger(&config.source)?;
        let filtered = source_completion::filter_entries_for_new_migration(
            mode,
            entries,
            &hashed_entries,
            &ledger,
        )?;
        state.skipped_completed_files = filtered.skipped_completed_files;
        return plan::build_plan_from_entries(filtered.entries_to_plan, options);
    }

    if !state.skipped_completed_files.is_empty() {
        let entries = scan::scan_source(&config.source)?;
        let mut hashing_progress = progress::TerminalProgress::new();
        let hashed_entries = source_completion::hash_source_entries_with_progress(
            &config.source,
            &entries,
            &mut hashing_progress,
        )?;
        let filtered_entries = source_completion::filter_entries_for_persisted_skips(
            mode,
            entries,
            &hashed_entries,
            &state.skipped_completed_files,
        )?;
        return plan::build_plan_from_entries(filtered_entries, options);
    }

    plan::build_plan(&config.source, options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::state::CompletedFileIdentity;
    use crate::models::state::PlannedBatch;
    use std::path::PathBuf;

    #[test]
    fn state_has_manifest_detects_presence_of_planned_batches() {
        let mut state = MigrationState::new("staging", "/src", "/dst");
        assert!(!state_has_manifest(&state));

        state.planned_batches.push(PlannedBatch {
            batch_id: "batch-000001".to_string(),
            file_count: 0,
            total_bytes: 0,
            files: Vec::new(),
        });
        assert!(state_has_manifest(&state));

        state.planned_batches.clear();
        state.skipped_completed_files.push(CompletedFileIdentity {
            mode: "staging".to_string(),
            relative_path: PathBuf::from("already-done.txt"),
            size_bytes: 1,
            blake3_hash: "hash".to_string(),
        });
        assert!(state_has_manifest(&state));
    }
}

pub(in crate::cli) fn execute_transfer(config: TransferConfig) -> Result<(), CaravanError> {
    let app_context = AppContext::new();
    let context = TransferContext::new(&config)?;

    let source_str = config.source.to_string_lossy().to_string();
    let dest_str = config.dest.to_string_lossy().to_string();
    let mode = mode_name(&config.mode);
    let state_filename = migration_registry::generate_state_filename(&source_str, &dest_str);

    let migration_id =
        register_migration(&app_context, &source_str, &dest_str, mode, &state_filename)?;

    let transfer_result = (|| -> Result<transfer_ops::TransferExecutionSummary, CaravanError> {
        let mut state = load_or_create_state(
            &config,
            &context.state_path,
            &context.secondary_state_path,
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

        print_state_save_locations(&context.state_path, &context.secondary_state_path);
        print_status_snapshot_policy(config.snapshot_every, config.snapshot_dir.as_deref());

        let plan_opts = PlanOptions {
            batch_size_bytes: config.batch_size_bytes,
            max_files: config.max_files.map(|v| v as usize),
        };
        let had_manifest = state_has_manifest(&state);
        let plan = build_plan_for_transfer_state(&config, &mut state, mode, &plan_opts)?;

        if had_manifest {
            plan::ensure_manifest_matches_snapshot(&state.planned_batches, &plan)?;
        } else {
            eprintln!(
                "[WARNING] State file has no immutable batch manifest; seeding from current source plan."
            );
            state.planned_batches = plan::planned_batches_from_snapshot(&plan);
        }

        let planning_summary = transfer_ops::summarize_transfer_plan(&plan);
        print_plan_summary(
            planning_summary.batch_count,
            planning_summary.source_file_count,
            planning_summary.source_total_bytes,
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
        context.persist_state(&state)?;
        warn_copy_backend_config(&config);

        let mut processed_batches = 0_u32;
        processed_batches += run_copy_phase(&context, &plan, &mut state)?;
        app_context.persist_migration_status(
            migration_id,
            migration_registry::MigrationStatus::Verifying,
        )?;
        processed_batches += run_verify_phase(&context, &plan, &mut state)?;
        state.migration_phase = MigrationPhase::AwaitingDeletion;
        context.persist_state(&state)?;
        app_context.persist_migration_status(
            migration_id,
            migration_registry::MigrationStatus::AwaitingDeletion,
        )?;
        run_delete_phase(&context, &mut state)?;
        let mut persist_state =
            |current_state: &MigrationState| context.persist_state(current_state);
        snapshot::process_pending_snapshots(
            config.mode.clone(),
            config.snapshot_every,
            &config.dest,
            config.snapshot_dir.as_deref(),
            &mut state,
            &context.snapshot_backend,
            &mut persist_state,
        )?;

        let execution_summary =
            transfer_ops::summarize_transfer_execution(&state, processed_batches);
        let final_phase = if execution_summary.pending_delete_batches > 0 {
            MigrationPhase::AwaitingDeletion
        } else {
            MigrationPhase::Completed
        };
        state.migration_phase = final_phase;
        context.persist_state(&state)?;
        print_migration_complete(
            execution_summary.processed_batches,
            execution_summary.completed_batches,
        );
        Ok(execution_summary)
    })();

    match &transfer_result {
        Ok(summary) => {
            let final_status = if summary.pending_delete_batches > 0 {
                migration_registry::MigrationStatus::AwaitingDeletion
            } else {
                migration_registry::MigrationStatus::Completed
            };
            app_context.persist_migration_status(migration_id, final_status)?;
        }
        Err(original_err) => {
            if let Err(status_err) = app_context
                .persist_migration_status(migration_id, migration_registry::MigrationStatus::Failed)
            {
                eprintln!(
                    "[WARNING] transfer failed and migration status could not be updated to failed: {}; original error: {}",
                    status_err, original_err
                );
            }
        }
    }

    transfer_result.map(|_| ())
}
