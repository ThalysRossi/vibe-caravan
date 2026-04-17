use std::fs;

use caravan::error::CaravanError;
use caravan::models::state::MigrationState;
use caravan::prompt::{BatchSizeMismatchChoice, PromptBackend};
use caravan::state_discovery::{check_state_file_compatibility, detect_state_file};
use caravan::state_store::{load_state, persist_state};
use tempfile::TempDir;

#[test]
fn detect_state_file_in_source_directory() {
    let source_dir = TempDir::new().expect("source temp dir");
    let dest_dir = TempDir::new().expect("dest temp dir");

    // Create source/.caravan directory
    let caravan_dir = source_dir.path().join(".caravan");
    fs::create_dir_all(&caravan_dir).expect("should create .caravan dir");

    // Create a state file in source directory
    let state_path = caravan_dir.join("state.json");
    let state = MigrationState::new(
        "staging",
        &source_dir.path().to_string_lossy(),
        &dest_dir.path().to_string_lossy(),
    );

    persist_state(&state_path, &state).expect("should persist state");

    // Test detection
    let detected =
        detect_state_file(source_dir.path(), dest_dir.path()).expect("should detect state file");
    assert_eq!(detected, state_path);
}

#[test]
fn detect_state_file_in_default_location() {
    let source_dir = TempDir::new().expect("source temp dir");
    let dest_dir = TempDir::new().expect("dest temp dir");

    // Create .caravan directory in current directory (relative to source)
    let caravan_dir = source_dir.path().join(".caravan");
    fs::create_dir_all(&caravan_dir).expect("should create .caravan dir");

    // Create a state file
    let state_path = caravan_dir.join("state.json");
    let state = MigrationState::new(
        "staging",
        &source_dir.path().to_string_lossy(),
        &dest_dir.path().to_string_lossy(),
    );

    persist_state(&state_path, &state).expect("should persist state");

    // Test detection
    let detected =
        detect_state_file(source_dir.path(), dest_dir.path()).expect("should detect state file");
    assert_eq!(detected, state_path);
}

#[test]
fn no_state_file_detected_when_missing() {
    let source_dir = TempDir::new().expect("source temp dir");
    let dest_dir = TempDir::new().expect("dest temp dir");

    // No state file exists
    let detected = detect_state_file(source_dir.path(), dest_dir.path());
    assert!(detected.is_none());
}

#[test]
fn detect_state_file_checks_batch_size_match() {
    let source_dir = TempDir::new().expect("source temp dir");
    let dest_dir = TempDir::new().expect("dest temp dir");

    // Create state file with batch size 1024
    let caravan_dir = source_dir.path().join(".caravan");
    fs::create_dir_all(&caravan_dir).expect("should create .caravan dir");

    let state_path = caravan_dir.join("state.json");
    let mut state = MigrationState::new(
        "staging",
        &source_dir.path().to_string_lossy(),
        &dest_dir.path().to_string_lossy(),
    );
    state.batch_size_bytes = 1024;

    persist_state(&state_path, &state).expect("should persist state");

    // Test with matching batch size
    let result = check_state_file_compatibility(&state_path, 1024);
    assert!(result.is_ok());

    // Test with mismatching batch size
    let result = check_state_file_compatibility(&state_path, 2048);
    assert!(result.is_err());
}

#[test]
fn handle_corrupted_state_file_gracefully() {
    let source_dir = TempDir::new().expect("source temp dir");
    let dest_dir = TempDir::new().expect("dest temp dir");

    // Create corrupted state file
    let caravan_dir = source_dir.path().join(".caravan");
    fs::create_dir_all(&caravan_dir).expect("should create .caravan dir");

    let state_path = caravan_dir.join("state.json");
    fs::write(&state_path, "invalid json content").expect("should write corrupt file");

    // Detection should still work (file exists)
    let detected = detect_state_file(source_dir.path(), dest_dir.path());
    assert!(detected.is_some());

    // But loading should fail
    let result = load_state(&state_path);
    assert!(result.is_err());
}

#[test]
fn prompt_user_when_batch_size_mismatch() {
    struct TestPrompt {
        choice: BatchSizeMismatchChoice,
    }

    impl PromptBackend for TestPrompt {
        fn confirm_deletion(&self, _batch_id: &str) -> Result<bool, CaravanError> {
            Ok(true)
        }

        fn confirm_batch_deletion(&self, _batch_ids: &[String]) -> Result<bool, CaravanError> {
            Ok(true)
        }

        fn ask_batch_size_mismatch(
            &self,
            _state_size: u64,
            _cli_size: u64,
        ) -> Result<BatchSizeMismatchChoice, CaravanError> {
            Ok(self.choice)
        }
    }

    // Test each choice
    let prompt = TestPrompt {
        choice: BatchSizeMismatchChoice::UseStateSize,
    };
    let result = prompt.ask_batch_size_mismatch(1024, 2048);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), BatchSizeMismatchChoice::UseStateSize);
}

#[test]
fn automatic_resume_detection_in_cli_parsing() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    // Create state file in source/.caravan
    let caravan_dir = source_dir.join(".caravan");
    fs::create_dir_all(&caravan_dir).expect("create .caravan");
    let state_path = caravan_dir.join("state.json");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024;

    persist_state(&state_path, &state).expect("persist state");
    let detected = detect_state_file(&source_dir, &dest_dir).expect("state should be detected");
    assert_eq!(
        detected, state_path,
        "automatic detection should prefer source/.caravan/state.json"
    );
}
