use std::fs;
use std::process::Command;

use caravan::migration_registry;
use caravan::models::state::{BatchPhase, BatchState, MigrationPhase, MigrationState};
use caravan::state_store::{load_state, persist_state};
use tempfile::TempDir;

fn run_staging(
    source_dir: &std::path::Path,
    dest_dir: &std::path::Path,
    cwd: &std::path::Path,
) -> std::process::Output {
    let binary = assert_cmd::cargo::cargo_bin("caravan");
    Command::new(binary)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "1GiB",
            "--skip-conflicts",
        ])
        .current_dir(cwd)
        .output()
        .expect("execute caravan")
}

fn count_files_in_dir(path: &std::path::Path) -> usize {
    fs::read_dir(path)
        .expect("read destination directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .count()
}

#[test]
fn staging_rerun_recovers_missing_files_from_legacy_verified_state() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    for index in 0..158 {
        let name = format!("file-{index:03}.txt");
        fs::write(source_dir.join(&name), format!("source-{index}")).expect("write source file");
        if index < 134 {
            fs::write(dest_dir.join(&name), format!("source-{index}"))
                .expect("write destination seed file");
        }
    }

    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024 * 1024 * 1024;
    state.migration_phase = MigrationPhase::AwaitingDeletion;
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::VerifyCompleted,
        verification_passed: true,
        approved_for_delete: false,
        deleted: false,
    });

    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    persist_state(&state_path, &state).expect("persist legacy-like state");

    let output = run_staging(&source_dir, &dest_dir, tmp.path());
    assert!(
        !output.status.success(),
        "non-interactive staging should still fail closed at deletion gate"
    );

    assert_eq!(
        count_files_in_dir(&dest_dir),
        158,
        "rerun should heal missing destination files even when legacy state says verify completed"
    );
}

#[test]
fn staging_rerun_copies_missing_files_even_when_batch_contains_conflicts() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    fs::write(source_dir.join("already-ok.txt"), "same").expect("write source already-ok");
    fs::write(source_dir.join("missing.txt"), "needs-copy").expect("write source missing");
    fs::write(source_dir.join("conflict.txt"), "source-version").expect("write source conflict");

    fs::write(dest_dir.join("already-ok.txt"), "same").expect("seed already-ok");
    fs::write(dest_dir.join("conflict.txt"), "dest-version").expect("seed conflicting file");

    let output = run_staging(&source_dir, &dest_dir, tmp.path());
    assert!(
        !output.status.success(),
        "run should still require operator review for unresolved conflicts"
    );

    assert_eq!(
        fs::read_to_string(dest_dir.join("missing.txt")).expect("missing file should be copied"),
        "needs-copy"
    );
    assert_eq!(
        fs::read_to_string(dest_dir.join("conflict.txt")).expect("conflicting file should exist"),
        "dest-version",
        "conflicting destination file should remain untouched by default"
    );
}

#[test]
fn subset_conflicts_do_not_skip_copy_for_non_conflicting_files_in_same_batch() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    fs::write(source_dir.join("conflict.txt"), "source-conflict").expect("write source conflict");
    fs::write(source_dir.join("missing-a.txt"), "copy-a").expect("write source missing-a");
    fs::write(source_dir.join("missing-b.txt"), "copy-b").expect("write source missing-b");

    fs::write(dest_dir.join("conflict.txt"), "dest-conflict").expect("seed conflict");

    let output = run_staging(&source_dir, &dest_dir, tmp.path());
    assert!(
        !output.status.success(),
        "run should stop at operator review when conflicts remain"
    );

    assert_eq!(
        fs::read_to_string(dest_dir.join("missing-a.txt")).expect("missing-a should be copied"),
        "copy-a"
    );
    assert_eq!(
        fs::read_to_string(dest_dir.join("missing-b.txt")).expect("missing-b should be copied"),
        "copy-b"
    );

    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    let state = load_state(&state_path).expect("load state after run");
    let batch = state
        .batch("batch-000001")
        .expect("single batch state should exist");
    assert_eq!(
        batch.phase,
        BatchPhase::Failed,
        "batch should remain failed while unresolved conflicts exist"
    );
}
