use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;
use caravan::models::state::{BatchPhase, BatchState, MigrationPhase, MigrationState};
use tempfile::TempDir;

pub struct SourceDestFixture {
    pub tmp: TempDir,
    pub source_dir: PathBuf,
    pub dest_dir: PathBuf,
}

pub fn source_dest_fixture() -> SourceDestFixture {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    SourceDestFixture {
        tmp,
        source_dir,
        dest_dir,
    }
}

pub fn migration_state_for_fixture(
    mode: &str,
    source_dir: &Path,
    dest_dir: &Path,
    batch_size_bytes: u64,
    migration_phase: MigrationPhase,
) -> MigrationState {
    let mut state = MigrationState::new(
        mode,
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = batch_size_bytes;
    state.migration_phase = migration_phase;
    state
}

pub fn batch_state(
    batch_id: &str,
    phase: BatchPhase,
    verification_passed: bool,
    approved_for_delete: bool,
    deleted: bool,
) -> BatchState {
    BatchState {
        batch_id: batch_id.to_string(),
        phase,
        verification_passed,
        approved_for_delete,
        deleted,
    }
}

#[allow(dead_code)]
pub fn run_resume(state_path: &Path, cwd: &Path) -> Output {
    let binary = assert_cmd::cargo::cargo_bin("caravan");
    Command::new(binary)
        .args([
            "resume",
            "--state",
            state_path.to_str().expect("utf8 state path"),
        ])
        .current_dir(cwd)
        .output()
        .expect("execute resume")
}

#[allow(dead_code)]
pub fn run_staging(source_dir: &Path, dest_dir: &Path, batch_size: &str, cwd: &Path) -> Output {
    let binary = assert_cmd::cargo::cargo_bin("caravan");
    Command::new(binary)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            batch_size,
        ])
        .current_dir(cwd)
        .output()
        .expect("execute caravan")
}
