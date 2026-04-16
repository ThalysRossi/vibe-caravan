use assert_cmd::Command;
use caravan::migration_registry::generate_state_filename;
use std::fs;
use tempfile::TempDir;

#[test]
fn state_file_saved_in_both_locations() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source dir");
    fs::create_dir_all(&dest_dir).expect("create dest dir");

    // Create a small test file
    fs::write(source_dir.join("test.txt"), "hello world").expect("write test file");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");

    // Run staging command
    let _output = Command::new(&binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().unwrap(),
            "--dest",
            dest_dir.to_str().unwrap(),
            "--batch-size",
            "1MiB",
            "--interactive",
        ])
        .current_dir(tmp.path())
        .output()
        .expect("Failed to execute caravan");

    // In interactive mode with no stdin, it should exit with error
    // But state should have been saved in both locations

    // Check primary location (source directory)
    let primary_state_dir = source_dir.join(".caravan");
    assert!(
        primary_state_dir.exists(),
        "Primary .caravan directory should exist in source"
    );

    let primary_state_files: Vec<_> = fs::read_dir(&primary_state_dir)
        .expect("read primary state dir")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
        .collect();

    assert!(
        !primary_state_files.is_empty(),
        "Primary state file should exist in source/.caravan/"
    );

    // Check secondary location (current directory)
    let secondary_state_dir = tmp.path().join(".caravan");
    assert!(
        secondary_state_dir.exists(),
        "Secondary .caravan directory should exist in current dir"
    );

    let secondary_state_file = secondary_state_dir.join("state.json");
    assert!(
        secondary_state_file.exists(),
        "Secondary state.json should exist in current/.caravan/"
    );

    // Both files should contain valid JSON
    for state_file in primary_state_files {
        let content = fs::read_to_string(state_file.path()).expect("read primary state file");
        let _: serde_json::Value =
            serde_json::from_str(&content).expect("parse primary state JSON");
    }

    let secondary_content =
        fs::read_to_string(&secondary_state_file).expect("read secondary state file");
    let _: serde_json::Value =
        serde_json::from_str(&secondary_content).expect("parse secondary state JSON");
}

#[test]
fn status_command_works_with_legacy_state_file() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source dir");
    fs::create_dir_all(&dest_dir).expect("create dest dir");

    // Create legacy state file in current directory
    let legacy_state_dir = tmp.path().join(".caravan");
    fs::create_dir_all(&legacy_state_dir).expect("create legacy .caravan dir");

    let legacy_state = serde_json::json!({
        "mode": "staging",
        "source": source_dir.to_string_lossy(),
        "destination": dest_dir.to_string_lossy(),
        "batch_size_bytes": 1048576,
        "last_successful_snapshot_name": null,
        "migration_phase": "Copying",
        "batches": [],
        "journal": []
    });

    fs::write(
        legacy_state_dir.join("state.json"),
        legacy_state.to_string(),
    )
    .expect("write legacy state file");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");

    // Status command should work with legacy state file
    let output = Command::new(&binary_path)
        .args(["status"])
        .current_dir(tmp.path())
        .output()
        .expect("Failed to execute caravan status");

    assert!(
        output.status.success(),
        "status should succeed with legacy state file"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Caravan Status"));
    assert!(stdout.contains("staging"));
}

#[test]
fn resume_command_works_with_primary_state_file() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source dir");
    fs::create_dir_all(&dest_dir).expect("create dest dir");

    // Create primary state file in source directory
    let primary_state_dir = source_dir.join(".caravan");
    fs::create_dir_all(&primary_state_dir).expect("create primary .caravan dir");

    let state_filename =
        generate_state_filename(&source_dir.to_string_lossy(), &dest_dir.to_string_lossy());

    let primary_state = serde_json::json!({
        "mode": "staging",
        "source": source_dir.to_string_lossy(),
        "destination": dest_dir.to_string_lossy(),
        "batch_size_bytes": 1048576,
        "last_successful_snapshot_name": null,
        "migration_phase": "Copying",
        "batches": [],
        "journal": []
    });

    fs::write(
        primary_state_dir.join(&state_filename),
        primary_state.to_string(),
    )
    .expect("write primary state file");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");

    // Resume command should work with primary state file (need to specify path)
    // Since resume defaults to .caravan/state.json, we need to pass the path
    let output = Command::new(&binary_path)
        .args([
            "resume",
            "--state",
            primary_state_dir.join(&state_filename).to_str().unwrap(),
        ])
        .current_dir(tmp.path())
        .output()
        .expect("Failed to execute caravan resume");

    assert!(
        output.status.success(),
        "resume should succeed for valid primary state path"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("No such file or directory"),
        "resume should not fail with file-not-found for explicit primary state path: {}",
        stderr
    );
}
