use std::path::Path;

use crate::cleanup;
use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::prompt;
use crate::signal::{ShutdownFlag, check_shutdown};

use super::{
    print_all_verified_banner, print_delete_source_batches_banner, print_deletion_not_approved,
};

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
        print_all_verified_banner(batches_needing_approval.len());

        let approved = prompt::request_approval_for_batches(
            Some(&prompt_backend),
            interactive,
            false,
            &batches_needing_approval,
        )?;

        if !approved {
            print_deletion_not_approved();
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

    print_delete_source_batches_banner(batch_ids.len());

    for batch_id in batch_ids {
        check_shutdown(shutdown_flag)?;

        if let Some(batch_state) = state.batch(batch_id) {
            if batch_state.deleted {
                continue;
            }

            let batch = state.materialize_planned_batch(batch_id).ok_or_else(|| {
                CaravanError::StateCorrupt(format!(
                    "missing immutable batch manifest for {}; cannot continue destructive step",
                    batch_id
                ))
            })?;
            let mut progress = crate::progress::TerminalProgress::new();
            let mut check_interrupt = || check_shutdown(shutdown_flag);
            cleanup::cleanup_batch_with_progress_and_interrupt(
                &batch,
                source_root,
                state,
                delete_context,
                &mut progress,
                &mut check_interrupt,
            )?;
            persist_state(state)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_batches_by_id_noops_for_empty_batch_list() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let mut state = MigrationState::new("staging", "/src", "/dst");
        let shutdown = ShutdownFlag::new();
        let mut persist_calls = 0usize;
        let mut persist_state = |_state: &MigrationState| {
            persist_calls += 1;
            Ok::<(), CaravanError>(())
        };

        delete_batches_by_id(
            &mut state,
            tmp.path(),
            &[],
            &shutdown,
            &mut persist_state,
            "unit-test",
        )
        .expect("empty delete list should be a no-op");

        assert_eq!(persist_calls, 0);
    }
}
