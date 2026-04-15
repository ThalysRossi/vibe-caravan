mod resume;
mod shared;
mod status;
mod transfer;

use std::path::Path;

use crate::config::TransferConfig;
use crate::error::CaravanError;

pub(super) fn execute_transfer(config: TransferConfig) -> Result<(), CaravanError> {
    transfer::execute_transfer(config)
}

pub(super) fn execute_status(state_path: &Path) -> Result<(), CaravanError> {
    status::execute_status(state_path)
}

pub(super) fn execute_resume(state_path: &Path, recover_failed: bool) -> Result<(), CaravanError> {
    resume::execute_resume(state_path, recover_failed)
}
