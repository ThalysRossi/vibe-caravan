use std::fs;

use assert_cmd::Command;
use caravan::models::state::{BatchPhase, BatchState, JournalEntry, MigrationState};
use caravan::state_store::persist_state;
use serde_json::Value;
use tempfile::TempDir;

#[test]
fn status_command_json_output_contains_expected_state_contract() {
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
    state.snapshot_every = Some(2);
    state.snapshot_dir = Some("/dst/snapshots".to_string());
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
            "--output",
            "json",
        ])
        .current_dir(tmp.path())
        .output()
        .expect("run status command");

    assert!(output.status.success(), "status should succeed");
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("parse JSON status output");

    assert_eq!(parsed["mode"], "staging");
    assert_eq!(parsed["source"], "/src");
    assert_eq!(parsed["destination"], "/dst");
    assert_eq!(parsed["snapshot_every"], 2);
    assert_eq!(parsed["snapshot_dir"], "/dst/snapshots");
    assert_eq!(parsed["last_successful_snapshot_name"], "snap-123");
    assert_eq!(parsed["batches"][0]["batch_id"], "batch-000001");
    assert_eq!(parsed["batches"][0]["phase"], "Planned");
    assert_eq!(parsed["batches"][0]["verification_passed"], false);
    assert_eq!(parsed["journal"][0]["event"], "copy_completed");
    assert_eq!(parsed["journal"][0]["batch_id"], "batch-000001");
    assert_eq!(parsed["journal"][0]["timestamp_unix_secs"], 123);
    assert_eq!(parsed["journal"][0]["context"], "test");
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
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stdout.contains("Snapshot cadence: disabled"));
    assert!(stdout.contains("=== All 1 batches have been verified successfully ==="));
    assert!(stdout.contains("=== Deleting source files for 1 batch(es) ==="));
    assert!(stdout.contains("=== Migration complete! 1 batches processed, 1 total completed ==="));
    assert!(
        stderr.contains("Done in"),
        "progress completion should be rendered to stderr, got: {stderr}"
    );
}

#[test]
fn transfer_interactive_multi_batch_decline_keeps_all_sources() {
    let tmp = TempDir::new().expect("create temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create destination");
    fs::write(source_dir.join("file1.txt"), "content-1").expect("write first source file");
    fs::write(source_dir.join("file2.txt"), "content-2").expect("write second source file");

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
            "--max-files",
            "1",
            "--interactive",
        ])
        .write_stdin("n\n")
        .current_dir(tmp.path())
        .output()
        .expect("run transfer command");

    assert!(
        output.status.success(),
        "interactive transfer should remain successful when operator declines deletion"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("All 2 batches have been verified successfully."));
    assert!(stdout.contains("Approve deletion of source files for ALL batches? (y/n):"));

    assert!(
        source_dir.join("file1.txt").exists() && source_dir.join("file2.txt").exists(),
        "source files should remain when deletion is declined"
    );
}

#[test]
fn transfer_interactive_multi_batch_accept_deletes_all_sources() {
    let tmp = TempDir::new().expect("create temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create destination");
    fs::write(source_dir.join("file1.txt"), "content-1").expect("write first source file");
    fs::write(source_dir.join("file2.txt"), "content-2").expect("write second source file");

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
            "--max-files",
            "1",
            "--interactive",
        ])
        .write_stdin("y\n")
        .current_dir(tmp.path())
        .output()
        .expect("run transfer command");

    assert!(
        output.status.success(),
        "interactive transfer should succeed when operator approves deletion"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("All 2 batches have been verified successfully."));
    assert!(stdout.contains("Approve deletion of source files for ALL batches? (y/n):"));
    assert!(stdout.contains("=== Deleting source files for 2 batch(es) ==="));

    assert!(
        !source_dir.join("file1.txt").exists() && !source_dir.join("file2.txt").exists(),
        "source files should be deleted when approval is granted"
    );
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
    assert!(stdout.contains("Snapshot cadence: disabled"));
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
            "--output",
            "json",
        ])
        .current_dir(tmp.path())
        .output()
        .expect("run resume inspect command");

    assert!(output.status.success(), "inspect mode should succeed");
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("parse JSON inspect output");
    assert_eq!(parsed["failed_batch_count"], 1);
    assert_eq!(parsed["failed_batches"][0]["batch_id"], "batch-000001");
    assert_eq!(
        parsed["failed_batches"][0]["all_destination_files_ready"],
        false
    );
    assert_eq!(
        parsed["failed_batches"][0]["missing_in_destination"][0],
        "file1.txt"
    );
    assert_eq!(
        parsed["failed_batches"][0]["size_mismatches"],
        Value::Array(vec![])
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

#[test]
fn resume_json_output_requires_inspect_failed_mode() {
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
            "--output",
            "json",
        ])
        .current_dir(tmp.path())
        .output()
        .expect("run resume with unsupported JSON output mode");

    assert!(
        !output.status.success(),
        "resume should reject JSON output when inspect-failed is not enabled"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("resume --output json is only supported with --inspect-failed"),
        "stderr should explain why JSON output is blocked, got: {stderr}"
    );
}
