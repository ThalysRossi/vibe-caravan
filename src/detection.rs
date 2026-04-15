use std::path::{Path, PathBuf};

use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::state_store::load_state;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateLocation {
    Source,
    Destination,
    CurrentDir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchSizeMismatchChoice {
    UseStateSize,
    EnterNewSize,
    StartFresh,
}

pub fn detect_state_file(source: &Path, dest: &Path) -> Option<PathBuf> {
    let source_state = source.join(".caravan/state.json");
    if source_state.exists() {
        return Some(source_state);
    }

    let dest_state = dest.join(".caravan/state.json");
    if dest_state.exists() {
        return Some(dest_state);
    }

    let current_state = PathBuf::from(".caravan/state.json");
    if current_state.exists() {
        return Some(current_state);
    }

    None
}
pub fn check_state_file_compatibility(
    state_path: &Path,
    cli_batch_size: u64,
) -> Result<MigrationState, CaravanError> {
    let state = load_state(state_path)?;

    if state.batch_size_bytes == 0 {
        return Ok(state);
    }

    if state.batch_size_bytes == cli_batch_size {
        Ok(state)
    } else {
        Err(CaravanError::InvalidArguments(format!(
            "Batch size mismatch: state has {} bytes, CLI specifies {} bytes",
            state.batch_size_bytes, cli_batch_size
        )))
    }
}

pub fn handle_batch_size_mismatch(
    prompt: &dyn crate::prompt::PromptBackend,
    state_batch_size: u64,
    cli_batch_size: u64,
) -> Result<BatchSizeMismatchChoice, CaravanError> {
    prompt.ask_batch_size_mismatch(state_batch_size, cli_batch_size)
}

pub fn parse_batch_size_input(input: &str) -> Result<u64, CaravanError> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(CaravanError::InvalidArguments(
            "batch-size cannot be empty".to_string(),
        ));
    }

    let split_idx = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
    let (number, unit_raw) = raw.split_at(split_idx);
    if number.is_empty() {
        return Err(CaravanError::InvalidArguments(
            "batch-size must start with digits".to_string(),
        ));
    }

    let base = number.parse::<u64>().map_err(|_| {
        CaravanError::InvalidArguments("batch-size numeric part is invalid".to_string())
    })?;
    let unit = unit_raw.trim().to_ascii_lowercase();

    let multiplier = match unit.as_str() {
        "" | "b" => 1_u64,
        "kib" => 1024_u64,
        "mib" => 1024_u64.pow(2),
        "gib" => 1024_u64.pow(3),
        "tib" => 1024_u64.pow(4),
        _ => {
            return Err(CaravanError::InvalidArguments(
                "unsupported batch-size unit; use B, KiB, MiB, GiB, or TiB".to_string(),
            ))
        }
    };

    base.checked_mul(multiplier)
        .ok_or_else(|| CaravanError::InvalidArguments("batch-size is too large".to_string()))
}

pub trait ExtendedPromptBackend: crate::prompt::PromptBackend {
    fn ask_batch_size_mismatch(
        &self,
        state_size: u64,
        cli_size: u64,
    ) -> Result<BatchSizeMismatchChoice, CaravanError>
    where
        Self: Sized,
    {
        handle_batch_size_mismatch(self, state_size, cli_size)
    }
}

impl<T: crate::prompt::PromptBackend + Sized> ExtendedPromptBackend for T {}
