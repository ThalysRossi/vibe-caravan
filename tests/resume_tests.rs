use std::fs;
use std::path::Path;

use caravan::cli::execute_transfer;
use caravan::config::{CopyStrategy, Mode, TransferConfig, VerificationMode};
use caravan::error::CaravanError;
use caravan::models::batch::Batch;
use caravan::models::file_entry::FileEntry;
use caravan::models::state::{BatchPhase, BatchState, MigrationState};
use caravan::resume::{
    classify_capacity_failure_message, classify_copy_failure_message,
    classify_verification_failure_message, load_state_for_resume, plan_resume_step,
    plan_resume_step_with_recovery, reconcile_batch_destination, recovery_message,
    require_delete_permission_for_resume, resume_run, FailureClass, ReconciliationResult,
    ResumeOptions, ResumeStepPlan,
};
use caravan::state_store::{load_state, persist_state};
use tempfile::TempDir;

fn sample_batch() -> Batch {
    Batch {
        id: "batch-000001".to_string(),
        file_count: 1,
        total_bytes: 3,
        files: vec![FileEntry {
            relative_path: "a.txt".into(),
            size_bytes: 3,
            modified_time: None,
        }],
    }
}

fn create_file(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent dirs");
    }
    fs::write(path, bytes).expect("write file");
}

#[test]
fn caravan_error_resume_variant_formats_message() {
    let err = CaravanError::Resume {
        class: "state_missing".to_string(),
        detail: "no file".to_string(),
    };
    let s = err.to_string();
    assert!(s.contains("resume"));
    assert!(s.contains("state_missing"));
    assert!(s.contains("no file"));
}

#[test]
fn recovery_message_covers_each_failure_class() {
    assert!(!recovery_message(FailureClass::StateMissing).is_empty());
    assert!(!recovery_message(FailureClass::StateCorrupted).is_empty());
    assert!(!recovery_message(FailureClass::StateFilesystemConflict).is_empty());
    assert!(!recovery_message(FailureClass::CopyBackendFailure).is_empty());
    assert!(!recovery_message(FailureClass::VerificationMismatch).is_empty());
    assert!(!recovery_message(FailureClass::CapacityExhausted).is_empty());
    assert!(!recovery_message(FailureClass::IoError).is_empty());
    assert!(!recovery_message(FailureClass::ResumePolicyBlocked).is_empty());
}

#[test]
fn failure_class_as_str_is_stable() {
    assert_eq!(FailureClass::StateMissing.as_str(), "state_missing");
}

#[test]
fn classify_helpers_return_expected_classes() {
    assert_eq!(
        classify_verification_failure_message("any"),
        FailureClass::VerificationMismatch
    );
    assert_eq!(
        classify_copy_failure_message("any"),
        FailureClass::CopyBackendFailure
    );
    assert_eq!(
        classify_capacity_failure_message("any"),
        FailureClass::CapacityExhausted
    );
}

#[test]
fn reconcile_batch_destination_detects_missing_and_size_mismatch() {
    let tmp = TempDir::new().expect("tmp");
    let batch = sample_batch();
    let recon = reconcile_batch_destination(&batch, tmp.path());
    assert!(!recon.all_destination_files_ready);
    assert_eq!(recon.missing_in_destination, vec!["a.txt"]);

    create_file(tmp.path(), "a.txt", b"xx");
    let recon2 = reconcile_batch_destination(&batch, tmp.path());
    assert!(!recon2.all_destination_files_ready);
    assert!(recon2.size_mismatches.contains(&"a.txt".to_string()));

    create_file(tmp.path(), "a.txt", b"abc");
    let recon3 = reconcile_batch_destination(&batch, tmp.path());
    assert!(recon3.all_destination_files_ready);
}

#[test]
fn resume_after_copy_before_verify_plans_verify() {
    let batch = sample_batch();
    let state = BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::CopyCompleted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    };
    let recon = ReconciliationResult {
        all_destination_files_ready: true,
        missing_in_destination: vec![],
        size_mismatches: vec![],
    };
    assert_eq!(
        plan_resume_step(&state, &recon, &batch),
        ResumeStepPlan::VerifyBatch
    );
}

#[test]
fn copy_started_with_partial_destination_plans_resume_copy() {
    let batch = sample_batch();
    let state = BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::CopyStarted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    };
    let recon = ReconciliationResult {
        all_destination_files_ready: false,
        missing_in_destination: vec!["a.txt".into()],
        size_mismatches: vec![],
    };
    assert_eq!(
        plan_resume_step(&state, &recon, &batch),
        ResumeStepPlan::CopyBatch
    );
}

#[test]
fn copy_started_with_complete_destination_plans_verify() {
    let batch = sample_batch();
    let state = BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::CopyStarted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    };
    let recon = ReconciliationResult {
        all_destination_files_ready: true,
        missing_in_destination: vec![],
        size_mismatches: vec![],
    };
    assert_eq!(
        plan_resume_step(&state, &recon, &batch),
        ResumeStepPlan::VerifyBatch
    );
}

#[test]
fn verify_completed_with_failed_verification_is_blocked() {
    let batch = sample_batch();
    let state = BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::VerifyCompleted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    };
    let recon = ReconciliationResult {
        all_destination_files_ready: true,
        missing_in_destination: vec![],
        size_mismatches: vec![],
    };
    assert_eq!(
        plan_resume_step(&state, &recon, &batch),
        ResumeStepPlan::BlockedFailedVerification
    );
}

#[test]
fn snapshot_completed_is_terminal() {
    let batch = sample_batch();
    let state = BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::SnapshotCompleted,
        verification_passed: true,
        approved_for_delete: true,
        deleted: true,
    };
    let recon = ReconciliationResult {
        all_destination_files_ready: true,
        missing_in_destination: vec![],
        size_mismatches: vec![],
    };
    assert_eq!(
        plan_resume_step(&state, &recon, &batch),
        ResumeStepPlan::BatchFullyCompleted
    );
}

#[test]
fn failed_phase_requires_operator_review() {
    let batch = sample_batch();
    let state = BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::Failed,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    };
    let recon = ReconciliationResult {
        all_destination_files_ready: false,
        missing_in_destination: vec![],
        size_mismatches: vec![],
    };
    assert!(matches!(
        plan_resume_step(&state, &recon, &batch),
        ResumeStepPlan::ConflictOperatorReview { .. }
    ));
}

#[test]
fn failed_phase_with_recovery_enabled_and_missing_files_plans_copy() {
    let batch = sample_batch();
    let state = BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::Failed,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    };
    let recon = ReconciliationResult {
        all_destination_files_ready: false,
        missing_in_destination: vec!["a.txt".into()],
        size_mismatches: vec![],
    };
    assert_eq!(
        plan_resume_step_with_recovery(&state, &recon, &batch, true),
        ResumeStepPlan::CopyBatch
    );
}

#[test]
fn failed_phase_with_recovery_enabled_and_complete_destination_plans_verify() {
    let batch = sample_batch();
    let state = BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::Failed,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    };
    let recon = ReconciliationResult {
        all_destination_files_ready: true,
        missing_in_destination: vec![],
        size_mismatches: vec![],
    };
    assert_eq!(
        plan_resume_step_with_recovery(&state, &recon, &batch, true),
        ResumeStepPlan::VerifyBatch
    );
}

#[test]
fn resume_after_verify_before_delete_plans_pending_approval() {
    let batch = sample_batch();
    let state = BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::VerifyCompleted,
        verification_passed: true,
        approved_for_delete: false,
        deleted: false,
    };
    let recon = ReconciliationResult {
        all_destination_files_ready: true,
        missing_in_destination: vec![],
        size_mismatches: vec![],
    };
    assert_eq!(
        plan_resume_step(&state, &recon, &batch),
        ResumeStepPlan::PendingDeleteApproval
    );
}

#[test]
fn resume_after_verify_with_approval_plans_delete_when_not_yet_deleted() {
    let batch = sample_batch();
    let state = BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::VerifyCompleted,
        verification_passed: true,
        approved_for_delete: true,
        deleted: false,
    };
    let recon = ReconciliationResult {
        all_destination_files_ready: true,
        missing_in_destination: vec![],
        size_mismatches: vec![],
    };
    assert_eq!(
        plan_resume_step(&state, &recon, &batch),
        ResumeStepPlan::DeleteSource
    );
}

#[test]
fn resume_after_delete_before_snapshot_plans_post_delete() {
    let batch = sample_batch();
    let state = BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::DeleteCompleted,
        verification_passed: true,
        approved_for_delete: true,
        deleted: true,
    };
    let recon = ReconciliationResult {
        all_destination_files_ready: true,
        missing_in_destination: vec![],
        size_mismatches: vec![],
    };
    assert_eq!(
        plan_resume_step(&state, &recon, &batch),
        ResumeStepPlan::PostDeleteSnapshot
    );
}

#[test]
fn resume_with_missing_state_file_fails_cleanly() {
    let tmp = TempDir::new().expect("tmp");
    let missing = tmp.path().join("no-state.json");
    let err = load_state_for_resume(&missing).expect_err("should fail");
    match err {
        CaravanError::Resume { class, detail } => {
            assert_eq!(class, FailureClass::StateMissing.as_str());
            assert!(detail.contains("does not exist"));
        }
        _ => panic!("expected CaravanError::Resume"),
    }
    let err2 = resume_run(&missing).expect_err("resume_run should fail same way");
    assert!(matches!(err2, CaravanError::Resume { .. }));
}

#[test]
fn resume_with_corrupted_state_fails_as_corrupted() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("bad.json");
    fs::write(&path, "{ not json").expect("write");
    let err = load_state_for_resume(&path).expect_err("parse should fail");
    match err {
        CaravanError::Resume { class, .. } => {
            assert_eq!(class, FailureClass::StateCorrupted.as_str());
        }
        _ => panic!("expected corrupted classification"),
    }
}

#[test]
fn resume_after_partial_batch_does_not_require_state_mutation_for_conflict() {
    let batch = sample_batch();
    let state = BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::CopyCompleted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    };
    let recon = ReconciliationResult {
        all_destination_files_ready: false,
        missing_in_destination: vec!["a.txt".into()],
        size_mismatches: vec![],
    };
    let plan = plan_resume_step(&state, &recon, &batch);
    assert!(matches!(
        plan,
        ResumeStepPlan::ConflictOperatorReview { .. }
    ));
}

#[test]
fn non_interactive_resume_without_approval_fails_closed_before_delete() {
    let state = BatchState {
        batch_id: "b1".into(),
        phase: BatchPhase::VerifyCompleted,
        verification_passed: true,
        approved_for_delete: false,
        deleted: false,
    };
    let opts = ResumeOptions {
        interactive: false,
        explicit_delete_approval: false,
    };
    let err = require_delete_permission_for_resume(&state, &opts).expect_err("blocked");
    match err {
        CaravanError::Resume { class, .. } => {
            assert_eq!(class, FailureClass::ResumePolicyBlocked.as_str());
        }
        _ => panic!("expected resume policy error"),
    }
}

#[test]
fn interactive_resume_allows_delete_gate_without_persisted_approval() {
    let state = BatchState {
        batch_id: "b1".into(),
        phase: BatchPhase::VerifyCompleted,
        verification_passed: true,
        approved_for_delete: false,
        deleted: false,
    };
    let opts = ResumeOptions {
        interactive: true,
        explicit_delete_approval: false,
    };
    require_delete_permission_for_resume(&state, &opts).expect("interactive ok");
}

#[test]
fn explicit_delete_approval_satisfies_resume_gate() {
    let state = BatchState {
        batch_id: "b1".into(),
        phase: BatchPhase::VerifyCompleted,
        verification_passed: true,
        approved_for_delete: false,
        deleted: false,
    };
    let opts = ResumeOptions {
        interactive: false,
        explicit_delete_approval: true,
    };
    require_delete_permission_for_resume(&state, &opts).expect("explicit ok");
}

#[test]
fn persisted_approval_satisfies_resume_gate_non_interactive() {
    let state = BatchState {
        batch_id: "b1".into(),
        phase: BatchPhase::ApprovedForDelete,
        verification_passed: true,
        approved_for_delete: true,
        deleted: false,
    };
    let opts = ResumeOptions {
        interactive: false,
        explicit_delete_approval: false,
    };
    require_delete_permission_for_resume(&state, &opts).expect("persisted approval ok");
}

#[test]
fn delete_gate_no_ops_when_batch_already_deleted() {
    let state = BatchState {
        batch_id: "b1".into(),
        phase: BatchPhase::DeleteCompleted,
        verification_passed: true,
        approved_for_delete: true,
        deleted: true,
    };
    let opts = ResumeOptions {
        interactive: false,
        explicit_delete_approval: false,
    };
    require_delete_permission_for_resume(&state, &opts).expect("deleted batch ok");
}

#[test]
fn delete_gate_blocks_when_verification_did_not_pass() {
    let state = BatchState {
        batch_id: "b1".into(),
        phase: BatchPhase::VerifyCompleted,
        verification_passed: false,
        approved_for_delete: true,
        deleted: false,
    };
    let opts = ResumeOptions {
        interactive: true,
        explicit_delete_approval: true,
    };
    let err = require_delete_permission_for_resume(&state, &opts).expect_err("verify must pass");
    assert!(matches!(err, CaravanError::Resume { .. }));
}

#[test]
fn resume_run_loads_valid_state() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("ok.json");
    let mut migration = MigrationState::new("staging", "/a", "/b");
    migration.upsert_batch(BatchState {
        batch_id: "batch-000001".into(),
        phase: BatchPhase::Planned,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });
    persist_state(&path, &migration).expect("persist");
    let loaded = resume_run(&path).expect("resume_run loads");
    assert_eq!(loaded.mode, "staging");
}

#[test]
fn end_to_end_state_round_trip_after_reconcile_conflict() {
    let tmp = TempDir::new().expect("tmp");
    let state_path = tmp.path().join("state.json");

    let batch = sample_batch();
    let batch_state = BatchState {
        batch_id: batch.id.clone(),
        phase: BatchPhase::CopyCompleted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    };

    let recon = ReconciliationResult {
        all_destination_files_ready: false,
        missing_in_destination: vec!["a.txt".into()],
        size_mismatches: vec![],
    };
    assert!(matches!(
        plan_resume_step(&batch_state, &recon, &batch),
        ResumeStepPlan::ConflictOperatorReview { .. }
    ));

    let mut migration = MigrationState::new("migrate", "/s", "/d");
    migration.upsert_batch(batch_state);
    persist_state(&state_path, &migration).expect("persist");

    let loaded = load_state_for_resume(&state_path).expect("load");
    assert_eq!(loaded.batches.len(), 1);
    assert_eq!(
        loaded.batch("batch-000001").unwrap().phase,
        BatchPhase::CopyCompleted
    );
}

#[test]
fn all_planned_batches_are_saved_in_state_before_processing() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    std::fs::create_dir_all(&source_dir).unwrap();
    std::fs::create_dir_all(&dest_dir).unwrap();

    // Create 3 test files that will be split into 3 batches
    for i in 0..3 {
        std::fs::write(
            source_dir.join(format!("file{}.txt", i)),
            format!("content {}", i),
        )
        .unwrap();
    }

    let config = TransferConfig {
        mode: Mode::Staging,
        source: source_dir.clone(),
        dest: dest_dir.clone(),
        batch_size_bytes: 10, // Small enough to force 3 separate batches
        max_files: None,
        snapshot_every: None,
        interactive: false,
        verification: VerificationMode::Structural,
        log_level: "error".to_string(),
        skip_conflicts: false,
        recover_failed: false,
        allow_unsafe_filesystems: false,
        copy_strategy: CopyStrategy::Auto,
        copy_buffer_size: TransferConfig::default_copy_buffer_size(),
        buffered_copy_threshold: TransferConfig::default_buffered_copy_threshold(),
    };

    // Run execute_transfer
    let _ = execute_transfer(config);

    // State should exist with ALL batches
    let state_path = Path::new(".caravan/state.json");
    assert!(state_path.exists(), "State file should exist");

    let state = load_state(state_path).expect("Failed to load state");

    // ✓ THE CRITICAL FIX: Verify we have ALL 3 batches in state, not just processed ones
    assert_eq!(
        state.batches.len(),
        3,
        "State must contain ALL planned batches, not only processed ones"
    );

    // Count how many batches completed processing
    let _completed_count = state
        .batches
        .iter()
        .filter(|b| b.phase == BatchPhase::VerifyCompleted || b.deleted)
        .count();

    // Regardless of how many completed, ALL batches must be present in state
    assert_eq!(
        state.batches.len(),
        3,
        "Even if execution stops early, all batches are in state"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(".caravan");
}
