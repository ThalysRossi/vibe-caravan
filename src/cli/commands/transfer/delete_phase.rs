use std::path::Path;

use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::signal::{check_shutdown, ShutdownFlag};

use super::super::shared::{
    approve_and_delete_verified_batches, ensure_no_operator_review_blocks,
    persist_state_both_locations,
};

pub(super) fn run_delete_phase(
    config: &TransferConfig,
    state: &mut MigrationState,
    state_path: &Path,
    secondary_state_path: &Path,
    shutdown_flag: &ShutdownFlag,
) -> Result<(), CaravanError> {
    check_shutdown(shutdown_flag)?;
    ensure_no_operator_review_blocks(state)?;

    let mut persist_state = |current_state: &MigrationState| {
        persist_state_both_locations(state_path, secondary_state_path, current_state)
    };
    approve_and_delete_verified_batches(
        state,
        &config.source,
        config.interactive,
        shutdown_flag,
        &mut persist_state,
        "execute_transfer",
    )?;

    Ok(())
}
