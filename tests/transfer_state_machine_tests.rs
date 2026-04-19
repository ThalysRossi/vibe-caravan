use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

use caravan::migration_registry;
use caravan::migration_registry::{MigrationRegistry, MigrationStatus};
use caravan::models::state::{BatchPhase, BatchState, MigrationState};
use caravan::state_store::persist_state;

fn first_state_file_in(dir: &std::path::Path) -> std::path::PathBuf {
    fs::read_dir(dir)
        .expect("read state dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .expect("state file should exist")
}

fn load_state_document(path: &std::path::Path) -> serde_json::Value {
    let doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(path).expect("read state file json"))
            .expect("parse state json");
    doc.get("state").cloned().unwrap_or(doc)
}

#[test]
fn transfer_success_marks_migration_registry_completed() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "source contents").expect("write source");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "1MiB",
            "--interactive",
        ])
        .write_stdin("y\n")
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(output.status.success(), "transfer should succeed");

    let registry_path = tmp.path().join(".caravan/migrations.json");
    let registry = MigrationRegistry::load(&registry_path).expect("load migration registry");
    assert_eq!(registry.migrations.len(), 1);
    assert_eq!(registry.migrations[0].status, MigrationStatus::Completed);

    let state_path = first_state_file_in(&source_dir.join(".caravan"));
    let state_json = load_state_document(&state_path);
    assert_eq!(
        state_json["migration_phase"],
        serde_json::Value::String("Completed".to_string())
    );
}

#[test]
fn transfer_failure_marks_migration_registry_failed() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let invalid_dest = tmp.path().join("missing-parent/dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::write(source_dir.join("file1.txt"), "source contents").expect("write source");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            invalid_dest.to_str().expect("utf8 dest"),
            "--batch-size",
            "1MiB",
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(!output.status.success(), "transfer should fail");

    let registry_path = tmp.path().join(".caravan/migrations.json");
    let registry = MigrationRegistry::load(&registry_path).expect("load migration registry");
    assert_eq!(registry.migrations.len(), 1);
    assert_eq!(registry.migrations[0].status, MigrationStatus::Failed);
}

#[test]
fn resume_success_marks_migration_registry_completed() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "source contents").expect("write source");
    fs::write(dest_dir.join("file1.txt"), "source contents").expect("write destination");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024 * 1024;
    state.upsert_planned_batch(caravan::models::state::PlannedBatch {
        batch_id: "batch-000001".to_string(),
        file_count: 1,
        total_bytes: "source contents".len() as u64,
        files: vec![caravan::models::state::PlannedFile {
            relative_path: std::path::PathBuf::from("file1.txt"),
            size_bytes: "source contents".len() as u64,
        }],
    });
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::ApprovedForDelete,
        verification_passed: true,
        approved_for_delete: true,
        deleted: false,
    });

    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    persist_state(&state_path, &state).expect("persist resume state");

    let registry_path = tmp.path().join(".caravan/migrations.json");
    let mut registry = MigrationRegistry::new();
    let migration_id = registry.add_migration(
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
        "staging",
        state_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("state filename"),
    );
    registry
        .update_status(migration_id, MigrationStatus::AwaitingDeletion)
        .expect("set initial status");
    registry.save(&registry_path).expect("save registry");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "resume",
            "--state",
            state_path.to_str().expect("utf8 state path"),
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan resume");

    assert!(output.status.success(), "resume should succeed");
    assert!(
        !source_dir.join("file1.txt").exists(),
        "resume should complete deletion for approved batch"
    );

    let registry = MigrationRegistry::load(&registry_path).expect("load migration registry");
    assert_eq!(registry.migrations.len(), 1);
    assert_eq!(registry.migrations[0].status, MigrationStatus::Completed);

    let state_json = load_state_document(&state_path);
    assert_eq!(
        state_json["migration_phase"],
        serde_json::Value::String("Completed".to_string())
    );
}

#[test]
fn resume_prefers_incomplete_registry_entry_when_history_contains_completed_entry() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "source contents").expect("write source");
    fs::write(dest_dir.join("file1.txt"), "source contents").expect("write destination");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024 * 1024;
    state.upsert_planned_batch(caravan::models::state::PlannedBatch {
        batch_id: "batch-000001".to_string(),
        file_count: 1,
        total_bytes: "source contents".len() as u64,
        files: vec![caravan::models::state::PlannedFile {
            relative_path: std::path::PathBuf::from("file1.txt"),
            size_bytes: "source contents".len() as u64,
        }],
    });
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::ApprovedForDelete,
        verification_passed: true,
        approved_for_delete: true,
        deleted: false,
    });

    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    persist_state(&state_path, &state).expect("persist resume state");

    let registry_path = tmp.path().join(".caravan/migrations.json");
    let mut registry = MigrationRegistry::new();
    let incomplete_id = registry.add_migration(
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
        "staging",
        state_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("state filename"),
    );
    registry
        .update_status(incomplete_id, MigrationStatus::AwaitingDeletion)
        .expect("set incomplete status");

    let completed_id = registry.add_migration(
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
        "staging",
        state_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("state filename"),
    );
    registry
        .update_status(completed_id, MigrationStatus::Completed)
        .expect("set completed status");
    registry.save(&registry_path).expect("save registry");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "resume",
            "--state",
            state_path.to_str().expect("utf8 state path"),
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan resume");

    assert!(output.status.success(), "resume should succeed");

    let registry = MigrationRegistry::load(&registry_path).expect("load migration registry");
    let incomplete = registry
        .find_by_id(incomplete_id)
        .expect("incomplete entry should exist");
    let completed = registry
        .find_by_id(completed_id)
        .expect("completed entry should exist");

    assert_eq!(incomplete.status, MigrationStatus::Completed);
    assert_eq!(completed.status, MigrationStatus::Completed);
}

#[test]
fn resume_updates_registry_entry_matching_state_file_when_multiple_incomplete_exist() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "source contents").expect("write source");
    fs::write(dest_dir.join("file1.txt"), "source contents").expect("write destination");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024 * 1024;
    state.upsert_planned_batch(caravan::models::state::PlannedBatch {
        batch_id: "batch-000001".to_string(),
        file_count: 1,
        total_bytes: "source contents".len() as u64,
        files: vec![caravan::models::state::PlannedFile {
            relative_path: std::path::PathBuf::from("file1.txt"),
            size_bytes: "source contents".len() as u64,
        }],
    });
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::ApprovedForDelete,
        verification_passed: true,
        approved_for_delete: true,
        deleted: false,
    });

    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    persist_state(&state_path, &state).expect("persist resume state");
    let state_file = state_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("state filename")
        .to_string();

    let registry_path = tmp.path().join(".caravan/migrations.json");
    let mut registry = MigrationRegistry::new();
    let correct_id = registry.add_migration(
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
        "staging",
        &state_file,
    );
    registry
        .update_status(correct_id, MigrationStatus::AwaitingDeletion)
        .expect("set correct entry status");

    let wrong_id = registry.add_migration(
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
        "staging",
        "legacy-state.json",
    );
    registry
        .update_status(wrong_id, MigrationStatus::AwaitingDeletion)
        .expect("set wrong entry status");
    registry.save(&registry_path).expect("save registry");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "resume",
            "--state",
            state_path.to_str().expect("utf8 state path"),
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan resume");

    assert!(output.status.success(), "resume should succeed");

    let registry = MigrationRegistry::load(&registry_path).expect("load migration registry");
    let correct = registry
        .find_by_id(correct_id)
        .expect("correct entry should exist");
    let wrong = registry
        .find_by_id(wrong_id)
        .expect("wrong entry should exist");
    assert_eq!(correct.status, MigrationStatus::Completed);
    assert_eq!(wrong.status, MigrationStatus::AwaitingDeletion);
}

#[test]
fn resume_without_deletion_approval_keeps_registry_at_awaiting_deletion() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "source contents").expect("write source");
    fs::write(dest_dir.join("file1.txt"), "source contents").expect("write destination");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024 * 1024;
    state.upsert_planned_batch(caravan::models::state::PlannedBatch {
        batch_id: "batch-000001".to_string(),
        file_count: 1,
        total_bytes: "source contents".len() as u64,
        files: vec![caravan::models::state::PlannedFile {
            relative_path: std::path::PathBuf::from("file1.txt"),
            size_bytes: "source contents".len() as u64,
        }],
    });
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::VerifyCompleted,
        verification_passed: true,
        approved_for_delete: false,
        deleted: false,
    });

    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    persist_state(&state_path, &state).expect("persist resume state");

    let registry_path = tmp.path().join(".caravan/migrations.json");
    let mut registry = MigrationRegistry::new();
    let migration_id = registry.add_migration(
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
        "staging",
        state_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("state filename"),
    );
    registry
        .update_status(migration_id, MigrationStatus::AwaitingDeletion)
        .expect("set initial status");
    registry.save(&registry_path).expect("save registry");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "resume",
            "--state",
            state_path.to_str().expect("utf8 state path"),
        ])
        .write_stdin("n\n")
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan resume");

    assert!(output.status.success(), "resume should succeed");
    assert!(
        source_dir.join("file1.txt").exists(),
        "source file should remain when deletion is not approved"
    );

    let registry = MigrationRegistry::load(&registry_path).expect("load migration registry");
    let entry = registry
        .find_by_id(migration_id)
        .expect("migration entry should exist");
    assert_eq!(entry.status, MigrationStatus::AwaitingDeletion);

    let state_json = load_state_document(&state_path);
    assert_eq!(
        state_json["migration_phase"],
        serde_json::Value::String("AwaitingDeletion".to_string())
    );
}

#[test]
fn transfer_without_deletion_approval_keeps_registry_at_awaiting_deletion() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "source contents").expect("write source");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "1MiB",
            "--interactive",
        ])
        .write_stdin("n\n")
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(output.status.success(), "transfer should succeed");
    assert!(
        source_dir.join("file1.txt").exists(),
        "source file should remain when deletion is not approved"
    );

    let registry_path = tmp.path().join(".caravan/migrations.json");
    let registry = MigrationRegistry::load(&registry_path).expect("load migration registry");
    assert_eq!(registry.migrations.len(), 1);
    assert_eq!(
        registry.migrations[0].status,
        MigrationStatus::AwaitingDeletion
    );

    let state_path = first_state_file_in(&source_dir.join(".caravan"));
    let state_json = load_state_document(&state_path);
    assert_eq!(
        state_json["migration_phase"],
        serde_json::Value::String("AwaitingDeletion".to_string())
    );
}

#[test]
fn skip_conflicts_marks_batch_failed_instead_of_copy_completed() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "source contents").expect("write source");
    fs::write(dest_dir.join("file1.txt"), "destination contents").expect("write conflicting dest");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "1MiB",
            "--skip-conflicts",
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(
        !output.status.success(),
        "run should stop for operator review after conflict skip"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("verification failed"),
        "conflict skip should not devolve into a verification error: {stderr}"
    );

    let state_path = first_state_file_in(&source_dir.join(".caravan"));
    let state_json = load_state_document(&state_path);

    assert_eq!(
        state_json["batches"][0]["phase"],
        serde_json::Value::String("Failed".to_string())
    );
    assert_eq!(state_json["batches"][0]["verification_passed"], false);
    let journal_entries = state_json["journal"]
        .as_array()
        .expect("journal must be array");
    assert!(
        journal_entries
            .iter()
            .any(|entry| entry["event"] == "copy_failed_conflict"),
        "state journal should include explicit conflict failure reason"
    );
}

#[test]
fn conflict_policy_skip_file_copies_non_conflicting_files_and_keeps_conflicts_for_review() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("conflict.txt"), "source-version").expect("write source conflict");
    fs::write(source_dir.join("missing.txt"), "needs-copy").expect("write source missing");
    fs::write(dest_dir.join("conflict.txt"), "dest-version").expect("write destination conflict");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "1MiB",
            "--conflict-policy",
            "skip-file",
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(
        !output.status.success(),
        "run should still require operator review when unresolved conflicts remain"
    );
    assert_eq!(
        fs::read_to_string(dest_dir.join("missing.txt")).expect("missing file should be copied"),
        "needs-copy"
    );
    assert_eq!(
        fs::read_to_string(dest_dir.join("conflict.txt"))
            .expect("conflicting destination file should remain"),
        "dest-version"
    );

    let state_path = first_state_file_in(&source_dir.join(".caravan"));
    let state_json = load_state_document(&state_path);
    assert_eq!(
        state_json["batches"][0]["phase"],
        serde_json::Value::String("Failed".to_string())
    );
}

#[test]
fn default_conflict_policy_copies_non_conflicting_files_and_keeps_conflicts_for_review() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("conflict.txt"), "source-version").expect("write source conflict");
    fs::write(source_dir.join("missing.txt"), "needs-copy").expect("write source missing");
    fs::write(dest_dir.join("conflict.txt"), "dest-version").expect("write destination conflict");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "1MiB",
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(
        !output.status.success(),
        "run should still require operator review when unresolved conflicts remain"
    );
    assert_eq!(
        fs::read_to_string(dest_dir.join("missing.txt")).expect("missing file should be copied"),
        "needs-copy"
    );
    assert_eq!(
        fs::read_to_string(dest_dir.join("conflict.txt"))
            .expect("conflicting destination file should remain"),
        "dest-version"
    );

    let state_path = first_state_file_in(&source_dir.join(".caravan"));
    let state_json = load_state_document(&state_path);
    assert_eq!(
        state_json["batches"][0]["phase"],
        serde_json::Value::String("Failed".to_string())
    );
}

#[test]
fn conflict_policy_skip_batch_never_overwrites_in_interactive_mode() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("conflict.txt"), "source-version").expect("write source conflict");
    fs::write(source_dir.join("missing.txt"), "needs-copy").expect("write source missing");
    fs::write(dest_dir.join("conflict.txt"), "dest-version").expect("write destination conflict");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "1MiB",
            "--interactive",
            "--conflict-policy",
            "skip-batch",
        ])
        .write_stdin("n\n")
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(
        !output.status.success(),
        "run should stop for operator review with skip-batch policy"
    );
    assert_eq!(
        fs::read_to_string(dest_dir.join("conflict.txt"))
            .expect("conflicting destination file should remain"),
        "dest-version"
    );
    assert!(
        !dest_dir.join("missing.txt").exists(),
        "skip-batch must not copy non-conflicting files"
    );

    let state_path = first_state_file_in(&source_dir.join(".caravan"));
    let state_json = load_state_document(&state_path);
    assert_eq!(
        state_json["batches"][0]["phase"],
        serde_json::Value::String("Failed".to_string())
    );
}

#[test]
fn existing_corrupted_state_is_not_replaced_by_new_state() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "content").expect("write source");

    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    fs::create_dir_all(state_path.parent().expect("state parent")).expect("create state dir");
    fs::write(&state_path, "{ not valid json").expect("write corrupt state");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "1MiB",
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(
        !output.status.success(),
        "corrupted existing state should fail closed"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to parse state file"),
        "stderr should surface the corrupt-state reason: {stderr}"
    );

    let final_contents = fs::read_to_string(&state_path).expect("read corrupt state after run");
    assert_eq!(final_contents, "{ not valid json");
}

#[test]
fn transfer_rerun_aborts_when_state_contains_failed_batch() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "content").expect("write source");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024 * 1024;
    state.batches.push(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::Failed,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });

    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    persist_state(&state_path, &state).expect("persist state");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "1MiB",
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(
        !output.status.success(),
        "transfer rerun should fail closed for failed batches"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("one or more batches require operator review before continuing"),
        "stderr should indicate operator-review gate: {stderr}"
    );
}

#[test]
fn transfer_rerun_with_recover_failed_retries_failed_batch() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "content").expect("write source");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024 * 1024;
    state.batches.push(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::Failed,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });

    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    persist_state(&state_path, &state).expect("persist state");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "1MiB",
            "--recover-failed",
            "--interactive",
        ])
        .write_stdin("n\n")
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(
        output.status.success(),
        "transfer rerun should recover failed batch when recover-failed is enabled"
    );
    assert!(
        dest_dir.join("file1.txt").exists(),
        "destination file should be copied during recovery"
    );

    let state_after = caravan::state_store::load_state(&state_path).expect("load updated state");
    let batch = state_after
        .batch("batch-000001")
        .expect("batch should exist in state");
    assert_eq!(batch.phase, BatchPhase::VerifyCompleted);
    assert!(batch.verification_passed);
}

#[test]
fn transfer_rerun_from_copy_started_retries_copy_without_conflict_skip() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "fresh source contents").expect("write source");
    fs::write(dest_dir.join("file1.txt"), "stale partial contents").expect("write stale dest");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024 * 1024;
    state.batches.push(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::CopyStarted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });

    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    persist_state(&state_path, &state).expect("persist state");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "1MiB",
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(
        !output.status.success(),
        "non-interactive run should still fail-closed at delete approval gate"
    );

    assert_eq!(
        fs::read_to_string(dest_dir.join("file1.txt")).expect("destination file should exist"),
        "fresh source contents"
    );

    let state_after = caravan::state_store::load_state(&state_path).expect("load updated state");
    let batch = state_after
        .batch("batch-000001")
        .expect("batch should exist in state");
    assert_eq!(batch.phase, BatchPhase::VerifyCompleted);
    assert!(batch.verification_passed);
}

#[test]
fn approved_for_delete_batches_are_not_recopied_on_transfer_rerun() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "content").expect("write source");
    fs::write(dest_dir.join("file1.txt"), "content").expect("write destination");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024 * 1024;
    state.batches.push(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::ApprovedForDelete,
        verification_passed: true,
        approved_for_delete: true,
        deleted: false,
    });

    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    persist_state(&state_path, &state).expect("persist state");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "1MiB",
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(
        output.status.success(),
        "approved-for-delete batches should skip copy and continue deletion"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Skipping batch-000001: copy already completed (phase: ApprovedForDelete)"),
        "rerun should explicitly skip copy for approved batch, got: {stdout}"
    );
    assert!(
        !stdout.contains("=== Copying batch-000001"),
        "rerun should not enter copy handler for approved batch, got: {stdout}"
    );
    assert!(
        !source_dir.join("file1.txt").exists(),
        "source file should be deleted for approved-for-delete batches"
    );
}

#[test]
fn approved_for_delete_batches_are_not_reverified_on_transfer_rerun() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "content").expect("write source");
    fs::write(dest_dir.join("file1.txt"), "content").expect("write destination");

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024 * 1024;
    state.batches.push(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::ApprovedForDelete,
        verification_passed: true,
        approved_for_delete: true,
        deleted: false,
    });

    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    persist_state(&state_path, &state).expect("persist state");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(&binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "1MiB",
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(
        output.status.success(),
        "approved-for-delete batches should skip verify and continue deletion"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "Skipping batch-000001: verification already completed (phase: ApprovedForDelete)"
        ),
        "rerun should explicitly skip verification for approved batch, got: {stdout}"
    );
    assert!(
        !stdout.contains("=== Verifying batch-000001"),
        "rerun should not enter verify handler for approved batch, got: {stdout}"
    );
    assert!(
        !source_dir.join("file1.txt").exists(),
        "source file should be deleted for approved-for-delete batches"
    );
}
