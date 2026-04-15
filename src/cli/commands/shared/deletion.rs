use std::path::Path;

use crate::cleanup;
use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::plan;
use crate::prompt;
use crate::signal::{check_shutdown, ShutdownFlag};

pub(crate) fn approve_and_delete_verified_batches(
    state: &mut MigrationState,
    source_root: &Path,
    interactive: bool,
    shutdown_flag: &ShutdownFlag,
    persist_state: &mut dyn FnMut(&MigrationState) -> Result<(), CaravanError>,
    delete_context: &str,
) -> Result<(), CaravanError> {
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
