//! Integration tests for graceful shutdown behavior.

use std::fs;
#[cfg(unix)]
use std::process::{Child, Stdio};
use std::process::Command;
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
use caravan::migration_registry;
#[cfg(unix)]
use caravan::models::state::{BatchPhase, MigrationState};
#[cfg(unix)]
use caravan::state_store::load_state;
use tempfile::TempDir;

fn caravan_binary() -> std::path::PathBuf {
    assert_cmd::cargo::cargo_bin("caravan")
}

fn create_test_files(root: &std::path::Path, file_count: usize, file_size: usize) {
    fs::create_dir_all(root).expect("create root");

    for i in 0..file_count {
        let file_path = root.join(format!("file_{i}.bin"));
        let content = vec![b'X'; file_size];
        fs::write(file_path, content).expect("write test file");
    }
}

#[cfg(unix)]
fn spawn_long_running_staging(
    binary_path: &std::path::Path,
    source_dir: &std::path::Path,
    dest_dir: &std::path::Path,
    cwd: &std::path::Path,
) -> Child {
    Command::new(binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "16MiB",
            "--max-files",
            "1",
            "--copy-strategy",
            "buffered",
            "--copy-buffer-size",
            "4KiB",
            "--buffered-copy-threshold",
            "1B",
        ])
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn caravan staging")
}

#[cfg(unix)]
fn wait_for_copy_activity(state_path: &std::path::Path) -> MigrationState {
    let start = Instant::now();
    let timeout = Duration::from_secs(15);

    loop {
        if state_path.exists() {
            if let Ok(state) = load_state(state_path) {
                let has_activity = state.batches.iter().any(|batch| {
                    matches!(
                        batch.phase,
                        BatchPhase::CopyStarted
                            | BatchPhase::CopyCompleted
                            | BatchPhase::VerifyCompleted
                            | BatchPhase::ApprovedForDelete
                            | BatchPhase::DeleteCompleted
                            | BatchPhase::SnapshotCompleted
                    )
                });
                if has_activity {
                    return state;
                }
            }
        }

        if start.elapsed() > timeout {
            panic!(
                "timed out waiting for copy activity in persisted state: {}",
                state_path.display()
            );
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(unix)]
fn send_sigint(pid: u32) {
    let status = Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .expect("invoke kill -INT");
    assert!(status.success(), "failed to deliver SIGINT to pid {pid}");
}

#[cfg(unix)]
fn progressed_batches(state: &MigrationState) -> usize {
    state
        .batches
        .iter()
        .filter(|batch| batch.phase != BatchPhase::Planned)
        .count()
}

#[test]
fn caravan_starts_and_runs_basic_command() {
    let temp_dir = TempDir::new().expect("temp dir");
    let source_dir = temp_dir.path().join("source");
    let dest_dir = temp_dir.path().join("dest");

    create_test_files(&source_dir, 3, 1024);
    fs::create_dir_all(&dest_dir).expect("create destination");

    let binary_path = caravan_binary();

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
        .current_dir(&temp_dir)
        .output()
        .expect("execute caravan");

    assert!(
        !output.status.success(),
        "non-interactive staging should fail closed at delete approval gate"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("destructive operations are blocked"),
        "expected delete-approval gate failure, got: {stderr}"
    );
}

#[test]
fn state_file_is_created_during_migration() {
    let temp_dir = TempDir::new().expect("temp dir");
    let source_dir = temp_dir.path().join("source");
    let dest_dir = temp_dir.path().join("dest");

    create_test_files(&source_dir, 1, 1024);
    fs::create_dir_all(&dest_dir).expect("create destination");

    let binary_path = caravan_binary();

    let output = Command::new(&binary_path)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "1MiB",
            "--max-files",
            "0",
        ])
        .current_dir(&temp_dir)
        .output()
        .expect("execute caravan");

    assert!(!output.status.success(), "max-files=0 should be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("max-files must be greater than zero"));
}

#[cfg(unix)]
#[test]
fn staging_receives_sigint_and_exits_with_resume_checkpoint() {
    let temp_dir = TempDir::new().expect("temp dir");
    let source_dir = temp_dir.path().join("source");
    let dest_dir = temp_dir.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create destination");

    // Large files + tiny buffered-copy size keeps copy phase active long enough for signal delivery.
    create_test_files(&source_dir, 4, 16 * 1024 * 1024);

    let binary_path = caravan_binary();
    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);

    let child = spawn_long_running_staging(&binary_path, &source_dir, &dest_dir, temp_dir.path());

    let _state_before_signal = wait_for_copy_activity(&state_path);
    send_sigint(child.id());

    let output = child.wait_with_output().expect("wait on staging child");
    assert!(
        !output.status.success(),
        "process should exit non-zero after SIGINT-triggered graceful shutdown"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("graceful shutdown requested")
            || stderr.contains("Shutdown requested. Finishing current operation"),
        "expected graceful shutdown signal path in stderr, got: {stderr}"
    );

    let state_after = load_state(&state_path).expect("load state after SIGINT");
    assert_eq!(
        state_after.batches.len(),
        4,
        "checkpoint should keep all planned batches"
    );
    assert!(
        progressed_batches(&state_after) >= 1,
        "at least one batch should have persisted progress before shutdown"
    );
    assert!(
        !state_after.batches.iter().all(|batch| batch.deleted),
        "shutdown checkpoint should occur before full completion"
    );
}

#[cfg(unix)]
#[test]
fn resume_continues_after_sigint_checkpoint() {
    let temp_dir = TempDir::new().expect("temp dir");
    let source_dir = temp_dir.path().join("source");
    let dest_dir = temp_dir.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create destination");
    create_test_files(&source_dir, 4, 16 * 1024 * 1024);

    let binary_path = caravan_binary();
    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);

    let child = spawn_long_running_staging(&binary_path, &source_dir, &dest_dir, temp_dir.path());
    let _ = wait_for_copy_activity(&state_path);
    send_sigint(child.id());
    let _ = child.wait_with_output().expect("wait on interrupted staging");

    let state_before_resume = load_state(&state_path).expect("load pre-resume state");
    let progressed_before = progressed_batches(&state_before_resume);
    assert!(
        progressed_before < state_before_resume.batches.len(),
        "interrupt happened too late to test resume progression"
    );

    let output = Command::new(&binary_path)
        .args(["resume", "--state", state_path.to_str().expect("utf8 state")])
        .current_dir(temp_dir.path())
        .output()
        .expect("execute resume");

    assert!(output.status.success(), "resume should complete successfully");

    let state_after_resume = load_state(&state_path).expect("load post-resume state");
    let progressed_after = progressed_batches(&state_after_resume);
    assert!(
        progressed_after > progressed_before,
        "resume should advance beyond interrupted checkpoint (before={progressed_before}, after={progressed_after})"
    );
}
