use std::fs;

use assert_cmd::Command;
use caravan::models::state::{BatchPhase, BatchState, JournalEntry, MigrationState};
use caravan::state_store::persist_state;
use tempfile::TempDir;

#[test]
fn status_command_output_includes_expected_sections() {
    let tmp = TempDir::new().expect("create temp dir");
    let state_path = tmp.path().join("state.json");

    let mut state = MigrationState::new("staging", "/src", "/dst");
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::Planned,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });
    state.last_successful_snapshot_name = Some("snap-123".to_string());
    state.journal.push(JournalEntry {
        event: "copy_completed".to_string(),
        batch_id: "batch-000001".to_string(),
        timestamp_unix_secs: 123,
        context: "test".to_string(),
    });
    persist_state(&state_path, &state).expect("persist state");

    let binary = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(binary)
        .args([
            "status",
            "--state",
            state_path.to_str().expect("state path utf8"),
        ])
        .current_dir(tmp.path())
        .output()
        .expect("run status command");

    assert!(output.status.success(), "status should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("=== Caravan Status ==="));
    assert!(stdout.contains("Mode: staging"));
    assert!(stdout.contains("Source: /src"));
    assert!(stdout.contains("Destination: /dst"));
    assert!(stdout.contains("Batches: 1"));
    assert!(stdout
        .contains("batch-000001 - Planned (verified: false, approved: false, deleted: false)"));
    assert!(stdout.contains("Last snapshot: snap-123"));
    assert!(stdout.contains("Journal entries: 1"));
    assert!(stdout.contains("[123] copy_completed - batch-000001 (test)"));
}

#[test]
fn transfer_interactive_output_includes_deletion_and_completion_banners() {
    let tmp = TempDir::new().expect("create temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create destination");
    fs::write(source_dir.join("file1.txt"), "content").expect("write source file");

    let binary = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(binary)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("source path utf8"),
            "--dest",
            dest_dir.to_str().expect("dest path utf8"),
            "--batch-size",
            "1MiB",
            "--interactive",
        ])
        .write_stdin("y\n")
        .current_dir(tmp.path())
        .output()
        .expect("run transfer command");

    assert!(
        output.status.success(),
        "interactive transfer should succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("=== All 1 batches have been verified successfully ==="));
    assert!(stdout.contains("=== Deleting source files for 1 batch(es) ==="));
    assert!(stdout.contains("=== Migration complete! 1 batches processed, 1 total completed ==="));
}

#[test]
fn resume_output_includes_header_and_completion_banner_for_approved_state() {
    let tmp = TempDir::new().expect("create temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    let state_path = tmp.path().join("resume-state.json");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create destination");
    fs::write(source_dir.join("file1.txt"), "content").expect("write source file");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024 * 1024;
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::ApprovedForDelete,
        verification_passed: true,
        approved_for_delete: true,
        deleted: false,
    });
    persist_state(&state_path, &state).expect("persist resume state");

    let binary = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(binary)
        .args([
            "resume",
            "--state",
            state_path.to_str().expect("state path utf8"),
        ])
        .current_dir(tmp.path())
        .output()
        .expect("run resume command");

    assert!(output.status.success(), "resume should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("=== Resuming from saved state ==="));
    assert!(stdout.contains("Completed batches: 0 / 1"));
    assert!(stdout.contains("=== Deleting source files for 1 batch(es) ==="));
    assert!(stdout.contains("✅ Resume complete! 1 batches processed, 1 total completed"));
}

#[test]
fn resume_inspect_failed_outputs_destination_diff_without_mutating_state() {
    let tmp = TempDir::new().expect("create temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    let state_path = tmp.path().join("resume-state.json");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create destination");
    fs::write(source_dir.join("file1.txt"), "content").expect("write source file");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024 * 1024;
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::Failed,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });
    persist_state(&state_path, &state).expect("persist resume state");

    let binary = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(binary)
        .args([
            "resume",
            "--state",
            state_path.to_str().expect("state path utf8"),
            "--inspect-failed",
        ])
        .current_dir(tmp.path())
        .output()
        .expect("run resume inspect command");

    assert!(output.status.success(), "inspect mode should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("=== Failed Batch Inspection ==="));
    assert!(stdout.contains("batch-000001"));
    assert!(stdout.contains("missing_in_destination"));
    assert!(stdout.contains("file1.txt"));
    assert!(
        !stdout.contains("Resuming transfer"),
        "inspect mode should not enter transfer execution"
    );

    let state_after = caravan::state_store::load_state(&state_path).expect("load state");
    let batch = state_after
        .batch("batch-000001")
        .expect("batch should still exist");
    assert_eq!(batch.phase, BatchPhase::Failed);
    assert!(
        !dest_dir.join("file1.txt").exists(),
        "inspect mode must not copy data"
    );
}
