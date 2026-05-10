use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use caravan::migration_registry;
use caravan::models::state::{
    BatchPhase, BatchState, CompletedFileIdentity, MigrationState, PlannedBatch, PlannedFile,
};
use caravan::source_completion;
use caravan::state_store::{load_state, persist_state};
use tempfile::TempDir;

fn run_staging(source: &Path, dest: &Path, cwd: &Path) -> std::process::Output {
    let binary = assert_cmd::cargo::cargo_bin("caravan");
    Command::new(binary)
        .args([
            "staging",
            "--source",
            source.to_str().expect("source path utf8"),
            "--dest",
            dest.to_str().expect("dest path utf8"),
            "--batch-size",
            "4B",
        ])
        .current_dir(cwd)
        .output()
        .expect("execute caravan staging")
}

fn planned_batch(batch_id: &str, relative_path: &str, size_bytes: u64) -> PlannedBatch {
    PlannedBatch {
        batch_id: batch_id.to_string(),
        file_count: 1,
        total_bytes: size_bytes,
        files: vec![PlannedFile {
            relative_path: PathBuf::from(relative_path),
            size_bytes,
        }],
    }
}

fn completed_batch_state(batch_id: &str) -> BatchState {
    BatchState {
        batch_id: batch_id.to_string(),
        phase: BatchPhase::CopyCompleted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    }
}

fn seed_completed_destination_state(source_dir: &Path, dest_dir: &Path) {
    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 4;
    state.upsert_planned_batch(planned_batch("batch-000001", "a.txt", 4));
    state.upsert_batch(completed_batch_state("batch-000001"));

    let state_path = migration_registry::state_file_in_source(source_dir, dest_dir);
    persist_state(&state_path, &state).expect("persist first destination state");
}

fn file_hash_hex(path: &Path) -> String {
    let hash = caravan::verify::digest_file(path).expect("hash file");
    let mut rendered = String::with_capacity(64);
    for byte in hash {
        rendered.push_str(&format!("{byte:02x}"));
    }
    rendered
}

#[test]
fn new_destination_skips_files_completed_by_previous_destination() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let first_dest = tmp.path().join("dest-1");
    let second_dest = tmp.path().join("dest-2");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&first_dest).expect("create first dest");
    fs::create_dir_all(&second_dest).expect("create second dest");
    fs::write(source_dir.join("a.txt"), "aaaa").expect("write source a");
    fs::write(source_dir.join("b.txt"), "bbbb").expect("write source b");

    seed_completed_destination_state(&source_dir, &first_dest);

    let output = run_staging(&source_dir, &second_dest, tmp.path());
    assert!(
        !output.status.success(),
        "non-interactive run should stop at delete approval after copying remaining files"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Hashing source"),
        "staging should show hashing progress before planning; stderr: {stderr}"
    );

    assert!(
        !second_dest.join("a.txt").exists(),
        "completed file from previous destination should not be copied again"
    );
    assert!(
        second_dest.join("b.txt").exists(),
        "remaining file should be copied to second destination"
    );

    let second_state_path = migration_registry::state_file_in_source(&source_dir, &second_dest);
    let second_state = load_state(&second_state_path).expect("load second destination state");
    assert_eq!(second_state.skipped_completed_files.len(), 1);
    assert_eq!(
        second_state.skipped_completed_files[0].relative_path,
        PathBuf::from("a.txt")
    );
    assert_eq!(second_state.planned_batches.len(), 1);
    assert_eq!(
        second_state.planned_batches[0].files[0].relative_path,
        PathBuf::from("b.txt")
    );

    let ledger = source_completion::load_ledger(&source_dir).expect("load source ledger");
    assert!(
        ledger
            .entries
            .iter()
            .any(|entry| entry.relative_path == PathBuf::from("a.txt")),
        "backfill should persist the previous completed source file"
    );
}

#[test]
fn rerun_fails_when_persisted_skipped_file_identity_changes() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("a.txt"), "aaaa").expect("write source a");
    fs::write(source_dir.join("b.txt"), "bbbb").expect("write source b");

    let skipped = CompletedFileIdentity {
        mode: "staging".to_string(),
        relative_path: PathBuf::from("a.txt"),
        size_bytes: 4,
        blake3_hash: file_hash_hex(&source_dir.join("a.txt")),
    };
    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 4;
    state.skipped_completed_files = vec![skipped];
    state.upsert_planned_batch(planned_batch("batch-000001", "b.txt", 4));
    state.upsert_batch(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::Planned,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });
    let state_path = migration_registry::state_file_in_source(&source_dir, &dest_dir);
    persist_state(&state_path, &state).expect("persist destination state");

    fs::write(source_dir.join("a.txt"), "zzzz").expect("mutate skipped source file");

    let output = run_staging(&source_dir, &dest_dir, tmp.path());
    assert!(
        !output.status.success(),
        "rerun should fail when skipped source file changes"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("source drift detected for skipped completed file"),
        "expected skipped-file drift error, got: {stderr}"
    );
}
