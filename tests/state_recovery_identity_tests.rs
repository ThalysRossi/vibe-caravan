use std::fs;

use assert_cmd::Command;
use caravan::migration_registry;
use caravan::models::state::{MigrationState, PlannedBatch, PlannedFile};
use caravan::state_store::{load_state, persist_state};
use tempfile::TempDir;

fn run_staging(
    source: &std::path::Path,
    dest: &std::path::Path,
    cwd: &std::path::Path,
) -> std::process::Output {
    let binary = assert_cmd::cargo::cargo_bin("caravan");
    Command::new(binary)
        .args([
            "staging",
            "--source",
            source.to_str().expect("source path utf8"),
            "--dest",
            dest.to_str().expect("dest path utf8"),
            "--batch-size",
            "1MiB",
        ])
        .current_dir(cwd)
        .output()
        .expect("run staging")
}

fn stale_manifest_state(
    mode: &str,
    source: &std::path::Path,
    dest: &std::path::Path,
) -> MigrationState {
    let mut state = MigrationState::new(mode, &source.to_string_lossy(), &dest.to_string_lossy());
    state.batch_size_bytes = 1024 * 1024;
    state.planned_batches = vec![
        PlannedBatch {
            batch_id: "batch-000001".to_string(),
            file_count: 1,
            total_bytes: 1,
            files: vec![PlannedFile {
                relative_path: "a.txt".into(),
                size_bytes: 1,
            }],
        },
        PlannedBatch {
            batch_id: "batch-000002".to_string(),
            file_count: 1,
            total_bytes: 1,
            files: vec![PlannedFile {
                relative_path: "b.txt".into(),
                size_bytes: 1,
            }],
        },
    ];
    state
}

#[test]
fn mismatched_compatibility_backup_is_ignored_when_canonical_state_is_missing() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    let other_source = tmp.path().join("other-source");
    let other_dest = tmp.path().join("other-dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create destination");
    fs::create_dir_all(&other_source).expect("create other source");
    fs::create_dir_all(&other_dest).expect("create other destination");
    fs::write(source_dir.join("file.txt"), "content").expect("write source file");

    let secondary_state_dir = tmp.path().join(".caravan");
    fs::create_dir_all(&secondary_state_dir).expect("create secondary state dir");
    let secondary_state_path = secondary_state_dir.join("state.json");
    let stale = stale_manifest_state("staging", &other_source, &other_dest);
    persist_state(&secondary_state_path, &stale).expect("persist stale secondary state");

    let output = run_staging(&source_dir, &dest_dir, tmp.path());

    assert!(
        !output.status.success(),
        "non-interactive staging should fail closed at delete gate"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("source drift detected"),
        "mismatched backup should be ignored, got stderr: {stderr}"
    );

    let canonical_state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    let canonical = load_state(&canonical_state_path).expect("canonical state should be created");
    assert_eq!(
        canonical.source,
        source_dir.to_string_lossy(),
        "new canonical state must match CLI source"
    );
    assert_eq!(
        canonical.destination,
        dest_dir.to_string_lossy(),
        "new canonical state must match CLI destination"
    );
}

#[test]
fn matching_compatibility_backup_is_loaded_when_canonical_state_is_missing() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create destination");
    fs::write(source_dir.join("file.txt"), "content").expect("write source file");

    let secondary_state_dir = tmp.path().join(".caravan");
    fs::create_dir_all(&secondary_state_dir).expect("create secondary state dir");
    let secondary_state_path = secondary_state_dir.join("state.json");
    let matching = stale_manifest_state("staging", &source_dir, &dest_dir);
    persist_state(&secondary_state_path, &matching).expect("persist matching secondary state");

    let output = run_staging(&source_dir, &dest_dir, tmp.path());

    assert!(
        !output.status.success(),
        "non-interactive staging should fail closed at delete gate"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Loaded existing state from: .caravan/state.json"),
        "load source should report compatibility backup path, got stdout: {stdout}"
    );
}

#[test]
fn mismatched_canonical_state_is_blocked_with_identity_error() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    let other_source = tmp.path().join("other-source");
    let other_dest = tmp.path().join("other-dest");

    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create destination");
    fs::create_dir_all(&other_source).expect("create other source");
    fs::create_dir_all(&other_dest).expect("create other destination");
    fs::write(source_dir.join("file.txt"), "content").expect("write source file");

    let canonical_state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    if let Some(parent) = canonical_state_path.parent() {
        fs::create_dir_all(parent).expect("create canonical state dir");
    }
    let mismatched = stale_manifest_state("staging", &other_source, &other_dest);
    persist_state(&canonical_state_path, &mismatched).expect("persist canonical mismatched state");

    let output = run_staging(&source_dir, &dest_dir, tmp.path());

    assert!(
        !output.status.success(),
        "mismatched canonical state must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("state identity mismatch"),
        "expected state identity mismatch error, got stderr: {stderr}"
    );
    assert!(
        !stderr.contains("source drift detected"),
        "identity mismatch should trigger before manifest drift check, got stderr: {stderr}"
    );
}
