use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::signal::check_shutdown;

use super::super::shared::{approve_and_delete_verified_batches, ensure_no_operator_review_blocks};
use super::context::TransferContext;

pub(super) fn run_delete_phase(
    context: &TransferContext<'_>,
    state: &mut MigrationState,
) -> Result<(), CaravanError> {
    check_shutdown(&context.shutdown_flag)?;
    ensure_no_operator_review_blocks(state)?;

    let mut persist_state = |current_state: &MigrationState| context.persist_state(current_state);
    approve_and_delete_verified_batches(
        state,
        &context.config.source,
        context.config.interactive,
        &context.shutdown_flag,
        &mut persist_state,
        "execute_transfer",
    )?;

    Ok(())
}
