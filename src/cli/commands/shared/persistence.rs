use std::path::Path;

use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::state_store;

/// Save state to both primary (source directory) and secondary (current directory) locations.
pub(crate) fn persist_state_both_locations(
    primary_path: &Path,
    secondary_path: &Path,
    state: &MigrationState,
) -> Result<(), CaravanError> {
    state_store::persist_state(primary_path, state)?;
    state_store::persist_state(secondary_path, state)?;
    Ok(())
}
