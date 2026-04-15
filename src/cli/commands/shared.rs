use std::path::Path;

use crate::capacity;
use crate::cleanup;
use crate::error::CaravanError;
use crate::models::state::{BatchPhase, MigrationState};
use crate::plan;
use crate::prompt;
use crate::signal::{check_shutdown, ShutdownFlag};
use crate::state_store;

/// Save state to both primary (source directory) and secondary (current directory) locations.
pub(super) fn persist_state_both_locations(
    primary_path: &Path,
    secondary_path: &Path,
    state: &MigrationState,
) -> Result<(), CaravanError> {
    // Save to primary location (source directory)
    state_store::persist_state(primary_path, state)?;

    // Save to secondary location (current directory) for backward compatibility
    state_store::persist_state(secondary_path, state)?;

    Ok(())
}

pub(super) fn ensure_destination_capacity(
    dest: &Path,
    required_bytes: u64,
) -> Result<(), CaravanError> {
    let capacity_report = capacity::check_capacity(dest, required_bytes, 0)?;
    if capacity_report.decision == capacity::CapacityDecision::Abort {
        eprintln!(
            "Capacity check failed: {}",
            capacity_report.reason.unwrap_or_default()
        );
        return Err(CaravanError::InvalidArguments(
            "insufficient destination space".to_string(),
        ));
    }
    Ok(())
}

fn failed_batches_requiring_review(state: &MigrationState) -> Vec<String> {
    state
        .batches
        .iter()
        .filter(|batch| batch.phase == BatchPhase::Failed && !batch.deleted)
        .map(|batch| batch.batch_id.clone())
        .collect()
}

fn failed_verification_batches_requiring_review(state: &MigrationState) -> Vec<String> {
    state
        .batches
        .iter()
        .filter(|batch| {
            !batch.deleted
                && !batch.verification_passed
                && matches!(
                    batch.phase,
                    BatchPhase::VerifyCompleted
                        | BatchPhase::ApprovedForDelete
                        | BatchPhase::DeleteCompleted
                        | BatchPhase::SnapshotCompleted
                )
        })
        .map(|batch| batch.batch_id.clone())
        .collect()
}

fn ensure_no_failed_batches(state: &MigrationState) -> Result<(), CaravanError> {
    let failed_batches = failed_batches_requiring_review(state);
    if !failed_batches.is_empty() {
        return Err(CaravanError::InvalidArguments(format!(
            "one or more batches require operator review before continuing: {}",
            failed_batches.join(", ")
        )));
    }
    Ok(())
}

fn ensure_no_failed_verification_batches(state: &MigrationState) -> Result<(), CaravanError> {
    let failed_verification_batches = failed_verification_batches_requiring_review(state);
    if !failed_verification_batches.is_empty() {
        return Err(CaravanError::InvalidArguments(format!(
            "one or more batches failed verification and require operator review before continuing: {}",
            failed_verification_batches.join(", ")
        )));
    }
    Ok(())
}

pub(super) fn ensure_no_operator_review_blocks(state: &MigrationState) -> Result<(), CaravanError> {
    ensure_no_failed_batches(state)?;
    ensure_no_failed_verification_batches(state)?;
    Ok(())
}

pub(super) fn approve_and_delete_verified_batches(
    state: &mut MigrationState,
    source_root: &Path,
    interactive: bool,
    shutdown_flag: &ShutdownFlag,
    persist_state: &mut dyn FnMut(&MigrationState) -> Result<(), CaravanError>,
    delete_context: &str,
) -> Result<(), CaravanError> {
    // If a prior run already approved deletion, continue deletion without prompting.
    let already_approved = state.batches_approved_but_not_deleted();
    delete_batches_by_id(
        state,
        source_root,
        &already_approved,
        shutdown_flag,
        persist_state,
        delete_context,
    )?;

    let batches_needing_approval = state.batches_needing_approval();
    if !batches_needing_approval.is_empty() {
        let prompt_backend = prompt::InteractivePrompt;
        println!(
            "\n=== All {} batches have been verified successfully ===",
            batches_needing_approval.len()
        );

        let approved = prompt::request_approval_for_batches(
            Some(&prompt_backend),
            interactive,
            false,
            &batches_needing_approval,
        )?;

        if !approved {
            println!("Deletion not approved. Stopping.");
            return Ok(());
        }

        state.approve_batches(&batches_needing_approval);
        persist_state(state)?;

        delete_batches_by_id(
            state,
            source_root,
            &batches_needing_approval,
            shutdown_flag,
            persist_state,
            delete_context,
        )?;
    }

    Ok(())
}

fn delete_batches_by_id(
    state: &mut MigrationState,
    source_root: &Path,
    batch_ids: &[String],
    shutdown_flag: &ShutdownFlag,
    persist_state: &mut dyn FnMut(&MigrationState) -> Result<(), CaravanError>,
    delete_context: &str,
) -> Result<(), CaravanError> {
    if batch_ids.is_empty() {
        return Ok(());
    }

    println!(
        "\n=== Deleting source files for {} batch(es) ===",
        batch_ids.len()
    );
    for batch_id in batch_ids {
        check_shutdown(shutdown_flag)?;

        if let Some(batch_state) = state.batch(batch_id) {
            if batch_state.deleted {
                continue;
            }
            let batch = plan::load_batch_definition(
                source_root,
                batch_id,
                state.batch_size_bytes,
                state.max_files,
            )?;
            cleanup::cleanup_batch(&batch, source_root, state, delete_context)?;
            persist_state(state)?;
        }
    }

    Ok(())
}
