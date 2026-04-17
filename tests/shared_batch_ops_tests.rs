use std::fs;
use std::path::PathBuf;

use caravan::error::CaravanError;
use caravan::models::batch::Batch;
use caravan::models::file_entry::FileEntry;
use caravan::models::state::{BatchPhase, BatchState, MigrationState};
use caravan::transfer::LocalFsCopyBackend;
use tempfile::TempDir;

mod error {
    pub use caravan::error::*;
}

mod models {
    pub mod batch {
        pub use caravan::models::batch::*;
    }

    pub mod state {
        pub use caravan::models::state::*;
    }

    pub mod verification {
        pub use caravan::models::verification::*;
    }
}

mod progress {
    pub use caravan::progress::*;
}

mod transfer {
    pub use caravan::transfer::*;
}

mod verify {
    pub use caravan::verify::*;
}

#[path = "../src/cli/commands/shared/batch_ops.rs"]
mod batch_ops;

use batch_ops::{copy_batch_with_state_updates, verify_batch_with_state_updates, CopyBatchOp};

fn single_file_batch(batch_id: &str, rel_path: &str, size_bytes: u64) -> Batch {
    Batch {
        id: batch_id.to_string(),
        files: vec![FileEntry {
            relative_path: PathBuf::from(rel_path),
            size_bytes,
            modified_time: None,
        }],
        total_bytes: size_bytes,
        file_count: 1,
    }
}

fn state_with_batch(
    batch_id: &str,
    phase: BatchPhase,
    verification_passed: bool,
) -> MigrationState {
    let mut state = MigrationState::new("migrate", "/src", "/dst");
    state.upsert_batch(BatchState {
        batch_id: batch_id.to_string(),
        phase,
        verification_passed,
        approved_for_delete: false,
        deleted: false,
    });
    state
}

fn write_file(dir: &TempDir, rel_path: &str, bytes: &[u8]) {
    let full = dir.path().join(rel_path);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).expect("failed to create parent directories");
    }
    fs::write(full, bytes).expect("failed to write test file");
}

#[test]
fn copy_batch_updates_phase_and_resets_verification_when_requested() {
    let src = TempDir::new().expect("failed to create src tempdir");
    let dst = TempDir::new().expect("failed to create dst tempdir");
    write_file(&src, "a/file.txt", b"hello");

    let batch = single_file_batch("batch-000001", "a/file.txt", 5);
    let mut state = state_with_batch(&batch.id, BatchPhase::Planned, true);
    let backend = LocalFsCopyBackend::new();

    let mut persisted = Vec::new();
    let mut persist = |current_state: &MigrationState| {
        persisted.push(current_state.clone());
        Ok::<(), CaravanError>(())
    };

    copy_batch_with_state_updates(
        &batch,
        &mut state,
        CopyBatchOp {
            source_root: src.path(),
            dest_root: dst.path(),
            copy_backend: &backend,
            reset_verification_passed: true,
        },
        &mut persist,
        &|batch_id| CaravanError::InvalidArguments(format!("missing state for {batch_id}")),
    )
    .expect("copy helper should succeed");

    assert_eq!(
        fs::read(dst.path().join("a/file.txt")).expect("dest file missing"),
        b"hello"
    );
    let final_state = state.batch(&batch.id).expect("batch missing after copy");
    assert_eq!(final_state.phase, BatchPhase::CopyCompleted);
    assert!(!final_state.verification_passed);
    assert_eq!(persisted.len(), 2);
    assert_eq!(
        persisted[0]
            .batch(&batch.id)
            .expect("batch missing in first persisted state")
            .phase,
        BatchPhase::CopyStarted
    );
    assert_eq!(
        persisted[1]
            .batch(&batch.id)
            .expect("batch missing in second persisted state")
            .phase,
        BatchPhase::CopyCompleted
    );
}

#[test]
fn copy_batch_can_preserve_existing_verification_flag() {
    let src = TempDir::new().expect("failed to create src tempdir");
    let dst = TempDir::new().expect("failed to create dst tempdir");
    write_file(&src, "file.txt", b"data");

    let batch = single_file_batch("batch-000001", "file.txt", 4);
    let mut state = state_with_batch(&batch.id, BatchPhase::CopyStarted, true);
    let backend = LocalFsCopyBackend::new();

    let mut persist = |_current_state: &MigrationState| Ok::<(), CaravanError>(());
    copy_batch_with_state_updates(
        &batch,
        &mut state,
        CopyBatchOp {
            source_root: src.path(),
            dest_root: dst.path(),
            copy_backend: &backend,
            reset_verification_passed: false,
        },
        &mut persist,
        &|batch_id| CaravanError::InvalidArguments(format!("missing state for {batch_id}")),
    )
    .expect("copy helper should succeed");

    let final_state = state.batch(&batch.id).expect("batch missing after copy");
    assert_eq!(final_state.phase, BatchPhase::CopyCompleted);
    assert!(final_state.verification_passed);
}

#[test]
fn copy_batch_returns_custom_error_when_batch_state_is_missing() {
    let src = TempDir::new().expect("failed to create src tempdir");
    let dst = TempDir::new().expect("failed to create dst tempdir");
    write_file(&src, "file.txt", b"data");

    let batch = single_file_batch("batch-000001", "file.txt", 4);
    let mut state = MigrationState::new("migrate", "/src", "/dst");
    let backend = LocalFsCopyBackend::new();

    let mut persist_calls = 0_u32;
    let mut persist = |_current_state: &MigrationState| {
        persist_calls += 1;
        Ok::<(), CaravanError>(())
    };
    let err = copy_batch_with_state_updates(
        &batch,
        &mut state,
        CopyBatchOp {
            source_root: src.path(),
            dest_root: dst.path(),
            copy_backend: &backend,
            reset_verification_passed: true,
        },
        &mut persist,
        &|batch_id| {
            CaravanError::InvalidArguments(format!("expected missing state for {batch_id}"))
        },
    )
    .expect_err("copy helper should fail when state is missing");

    assert_eq!(
        err.to_string(),
        "invalid arguments: expected missing state for batch-000001"
    );
    assert_eq!(persist_calls, 0);
}

#[test]
fn verify_batch_marks_batch_verified_on_success() {
    let src = TempDir::new().expect("failed to create src tempdir");
    let dst = TempDir::new().expect("failed to create dst tempdir");
    write_file(&src, "file.txt", b"hello");
    write_file(&dst, "file.txt", b"hello");

    let batch = single_file_batch("batch-000001", "file.txt", 5);
    let mut state = state_with_batch(&batch.id, BatchPhase::CopyCompleted, false);

    let mut persisted = Vec::new();
    let mut persist = |current_state: &MigrationState| {
        persisted.push(current_state.clone());
        Ok::<(), CaravanError>(())
    };
    verify_batch_with_state_updates(
        &batch,
        src.path(),
        dst.path(),
        &mut state,
        &mut persist,
        &|batch_id| CaravanError::InvalidArguments(format!("missing state for {batch_id}")),
    )
    .expect("verify helper should succeed");

    let final_state = state.batch(&batch.id).expect("batch missing after verify");
    assert_eq!(final_state.phase, BatchPhase::VerifyCompleted);
    assert!(final_state.verification_passed);
    assert_eq!(persisted.len(), 1);
}

#[test]
fn verify_batch_returns_error_and_records_failed_verification() {
    let src = TempDir::new().expect("failed to create src tempdir");
    let dst = TempDir::new().expect("failed to create dst tempdir");
    write_file(&src, "file.txt", b"aaaa");
    write_file(&dst, "file.txt", b"bbbb");

    let batch = single_file_batch("batch-000001", "file.txt", 4);
    let mut state = state_with_batch(&batch.id, BatchPhase::CopyCompleted, true);

    let mut persist_calls = 0_u32;
    let mut persist = |_current_state: &MigrationState| {
        persist_calls += 1;
        Ok::<(), CaravanError>(())
    };
    let err = verify_batch_with_state_updates(
        &batch,
        src.path(),
        dst.path(),
        &mut state,
        &mut persist,
        &|batch_id| CaravanError::InvalidArguments(format!("missing state for {batch_id}")),
    )
    .expect_err("verify helper should fail on digest mismatch");

    assert_eq!(
        err.to_string(),
        "verification failed: Verification failed; stop and require human review before deletion."
    );
    let final_state = state.batch(&batch.id).expect("batch missing after verify");
    assert_eq!(final_state.phase, BatchPhase::VerifyCompleted);
    assert!(!final_state.verification_passed);
    assert_eq!(persist_calls, 1);
}
