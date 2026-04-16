use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::state::{BatchPhase, JournalEntry, MigrationState};
use crate::prompt::PromptBackend;
use crate::{prompt, transfer};

use super::super::shared::{
    copy_batch_with_state_updates, ensure_destination_capacity, persist_state_both_locations,
    print_copy_batch_banner, print_verification_passed, print_verify_batch_banner,
    verify_batch_with_state_updates, CopyBatchOp,
};
use super::setup::planned_batch_state;

pub(super) fn copy_single_batch(
    batch: &Batch,
    config: &TransferConfig,
    state: &mut MigrationState,
    state_path: &Path,
    secondary_state_path: &Path,
    copy_backend: &transfer::LocalFsCopyBackend,
) -> Result<(), CaravanError> {
    print_copy_batch_banner(batch);

    let mut batch_state = state
        .batch(&batch.id)
        .cloned()
        .unwrap_or_else(|| planned_batch_state(&batch.id));

    ensure_destination_capacity(&config.dest, batch.total_bytes)?;

    let requires_conflict_check =
        matches!(batch_state.phase, BatchPhase::Planned | BatchPhase::Failed);
    if requires_conflict_check {
        let conflict_report = crate::conflict::detect_batch_conflicts(batch, &config.dest)?;
        if conflict_report.has_conflicts {
            let should_skip = if config.skip_conflicts || !config.interactive {
                true
            } else {
                let prompt_backend = prompt::InteractivePrompt;
                prompt_backend.confirm_conflict_skip(&batch.id, &conflict_report)?
            };

            if should_skip {
                println!(
                    "⚠️  Skipping batch '{}' due to {} naming conflict(s)",
                    batch.id, conflict_report.total_conflicts
                );

                batch_state.phase = BatchPhase::Failed;
                batch_state.verification_passed = false;
                state.upsert_batch(batch_state);
                state.journal.push(JournalEntry {
                    event: "copy_failed_conflict".to_string(),
                    batch_id: batch.id.clone(),
                    timestamp_unix_secs: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    context: format!(
                        "naming_conflicts={} size_mismatches={}",
                        conflict_report.total_conflicts,
                        conflict_report.size_mismatches.len()
                    ),
                });
                persist_state_both_locations(state_path, secondary_state_path, state)?;
                return Ok(());
            }
        }
    }

    state.upsert_batch(batch_state);
    let mut persist_state = |current_state: &MigrationState| {
        persist_state_both_locations(state_path, secondary_state_path, current_state)
    };
    copy_batch_with_state_updates(
        batch,
        state,
        CopyBatchOp {
            source_root: &config.source,
            dest_root: &config.dest,
            copy_backend,
            reset_verification_passed: true,
        },
        &mut persist_state,
        &|batch_id| {
            CaravanError::InvalidArguments(format!(
                "batch {} disappeared from state during copy",
                batch_id
            ))
        },
    )?;

    Ok(())
}

pub(super) fn verify_single_batch(
    batch: &Batch,
    config: &TransferConfig,
    state: &mut MigrationState,
    state_path: &Path,
    secondary_state_path: &Path,
) -> Result<(), CaravanError> {
    print_verify_batch_banner(batch);

    let mut persist_state = |current_state: &MigrationState| {
        persist_state_both_locations(state_path, secondary_state_path, current_state)
    };
    verify_batch_with_state_updates(
        batch,
        &config.source,
        &config.dest,
        state,
        &mut persist_state,
        &|batch_id| {
            CaravanError::InvalidArguments(format!(
                "missing batch state for {} before verification",
                batch_id
            ))
        },
    )?;

    print_verification_passed();
    Ok(())
}
