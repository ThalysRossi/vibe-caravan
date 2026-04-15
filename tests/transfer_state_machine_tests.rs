use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

use caravan::migration_registry;

fn first_state_file_in(dir: &std::path::Path) -> std::path::PathBuf {
    fs::read_dir(dir)
        .expect("read state dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .expect("state file should exist")
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
    let state_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("read state"))
            .expect("parse state");

    assert_eq!(
        state_json["batches"][0]["phase"],
        serde_json::Value::String("Failed".to_string())
    );
    assert_eq!(state_json["batches"][0]["verification_passed"], false);
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
