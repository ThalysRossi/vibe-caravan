use std::path::{Path, PathBuf};

use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::state_store::load_state;

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

    if state.batch_size_bytes == 0 || state.batch_size_bytes == cli_batch_size {
        return Ok(state);
    }

    Err(CaravanError::InvalidArguments(format!(
        "Batch size mismatch: state has {} bytes, CLI specifies {} bytes",
        state.batch_size_bytes, cli_batch_size
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    use crate::state_store::persist_state;

    #[test]
    fn detect_state_file_prefers_source_then_destination() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let dest = temp.path().join("dest");
        std::fs::create_dir_all(source.join(".caravan")).expect("create source caravan");
        std::fs::create_dir_all(dest.join(".caravan")).expect("create dest caravan");

        let source_state = source.join(".caravan/state.json");
        let dest_state = dest.join(".caravan/state.json");
        std::fs::write(&source_state, b"{}").expect("seed source state");
        std::fs::write(&dest_state, b"{}").expect("seed dest state");

        let detected = detect_state_file(&source, &dest).expect("state file expected");
        assert_eq!(detected, source_state);
    }

    #[test]
    fn detect_state_file_falls_back_to_destination() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let dest = temp.path().join("dest");
        std::fs::create_dir_all(dest.join(".caravan")).expect("create dest caravan");
        let dest_state = dest.join(".caravan/state.json");
        std::fs::write(&dest_state, b"{}").expect("seed dest state");

        let detected = detect_state_file(&source, &dest).expect("state file expected");
        assert_eq!(detected, dest_state);
    }

    #[test]
    fn detect_state_file_falls_back_to_current_working_directory_state_file() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let dest = temp.path().join("dest");
        let detected = detect_state_file(&source, &dest);
        if let Some(path) = detected {
            assert_eq!(path, PathBuf::from(".caravan/state.json"));
        }
    }

    #[test]
    fn check_state_file_compatibility_accepts_matching_and_zero_batch_size() {
        let temp = tempdir().expect("tempdir");
        let state_path = temp.path().join("state.json");

        let mut state = MigrationState::new("migrate", "/src", "/dst");
        state.batch_size_bytes = 1024;
        persist_state(&state_path, &state).expect("persist state");
        let loaded =
            check_state_file_compatibility(&state_path, 1024).expect("matching batch size");
        assert_eq!(loaded.batch_size_bytes, 1024);

        state.batch_size_bytes = 0;
        persist_state(&state_path, &state).expect("persist state with zero");
        let loaded =
            check_state_file_compatibility(&state_path, 999).expect("zero state must be accepted");
        assert_eq!(loaded.batch_size_bytes, 0);
    }

    #[test]
    fn check_state_file_compatibility_rejects_mismatched_batch_size() {
        let temp = tempdir().expect("tempdir");
        let state_path = temp.path().join("state.json");
        let mut state = MigrationState::new("migrate", "/src", "/dst");
        state.batch_size_bytes = 4096;
        persist_state(&state_path, &state).expect("persist state");

        let err =
            check_state_file_compatibility(&state_path, 1024).expect_err("mismatch should fail");
        let rendered = err.to_string();
        assert!(
            rendered
                .contains("Batch size mismatch: state has 4096 bytes, CLI specifies 1024 bytes"),
            "unexpected error: {rendered}"
        );
    }
}
