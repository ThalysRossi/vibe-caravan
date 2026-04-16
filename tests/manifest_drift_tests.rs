use std::fs;

use assert_cmd::Command;
use caravan::migration_registry;
use caravan::state_store::load_state;
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
        .expect("execute staging")
}

#[test]
fn transfer_rerun_fails_when_source_drift_detected_against_manifest() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source dir");
    fs::create_dir_all(&dest_dir).expect("create destination dir");
    fs::write(source_dir.join("file1.txt"), "content").expect("write source file");

    // First run seeds state/manifest and fails closed at delete-approval gate.
    let first = run_staging(&source_dir, &dest_dir, tmp.path());
    assert!(
        !first.status.success(),
        "non-interactive run should fail closed at delete approval"
    );

    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    let state = load_state(&state_path).expect("state should be persisted");
    assert!(
        !state.planned_batches.is_empty(),
        "state should include immutable manifest"
    );

    // Mutate source after manifest was persisted.
    fs::write(source_dir.join("new-file.txt"), "new").expect("write drift file");

    let rerun = run_staging(&source_dir, &dest_dir, tmp.path());
    assert!(
        !rerun.status.success(),
        "rerun should fail when source drifts from immutable manifest"
    );
    let stderr = String::from_utf8_lossy(&rerun.stderr);
    assert!(
        stderr.contains("source drift detected"),
        "expected source drift error, got: {stderr}"
    );
}

#[test]
fn resume_fails_when_source_drift_detected_against_manifest() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source dir");
    fs::create_dir_all(&dest_dir).expect("create destination dir");
    fs::write(source_dir.join("file1.txt"), "content").expect("write source file");

    let first = run_staging(&source_dir, &dest_dir, tmp.path());
    assert!(
        !first.status.success(),
        "non-interactive run should fail closed at delete approval"
    );

    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    let state = load_state(&state_path).expect("state should be persisted");
    assert!(
        !state.planned_batches.is_empty(),
        "state should include immutable manifest"
    );

    // Source drift after planning should block resume before batch processing.
    fs::write(source_dir.join("extra.txt"), "drift").expect("write drift file");

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
        .expect("execute resume");

    assert!(
        !output.status.success(),
        "resume should fail when source drift is detected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("source drift detected"),
        "expected source drift error, got: {stderr}"
    );
}
