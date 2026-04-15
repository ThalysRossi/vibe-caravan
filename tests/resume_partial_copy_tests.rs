use caravan::models::state::{BatchPhase, BatchState, MigrationPhase, MigrationState};
use caravan::state_store::persist_state;
use std::fs;
use tempfile::TempDir;

#[test]
fn resume_skips_already_copied_batches_and_verifies() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    // Create some test files
    fs::write(source_dir.join("file1.txt"), "content1").expect("create file1");
    fs::write(source_dir.join("file2.txt"), "content2").expect("create file2");
    fs::write(source_dir.join("file3.txt"), "content3").expect("create file3");

    // Create state with partial copy completion
    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
    );
    state.batch_size_bytes = 1024;
    state.migration_phase = MigrationPhase::Copying;

    // Batch 1: CopyCompleted, not verified
    state.batches.push(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::CopyCompleted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });

    // Batch 2: CopyCompleted, not verified
    state.batches.push(BatchState {
        batch_id: "batch-000002".to_string(),
        phase: BatchPhase::CopyCompleted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });

    // Batch 3: Planned (not started)
    state.batches.push(BatchState {
        batch_id: "batch-000003".to_string(),
        phase: BatchPhase::Planned,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });

    // Create state directory and save state
    let state_dir = tmp.path().join(".caravan");
    fs::create_dir_all(&state_dir).expect("create .caravan");
    let state_path = state_dir.join("state.json");
    persist_state(&state_path, &state).expect("persist state");

    // Note: This test would need to mock the actual copy/verify operations
    // For now, we'll test the resume logic indirectly
    // The key assertion: resume should NOT copy batch-000001 or batch-000002
    // since they're already CopyCompleted

    // Test the plan_resume_step logic directly
    use caravan::models::batch::Batch;
    use caravan::resume::{plan_resume_step, ReconciliationResult};

    let recon = ReconciliationResult {
        all_destination_files_ready: true,
        missing_in_destination: vec![],
        size_mismatches: vec![],
    };

    let dummy_batch = Batch {
        id: "dummy".to_string(),
        files: vec![],
        total_bytes: 0,
        file_count: 0,
    };

    // Test batch 1 (CopyCompleted) -> should plan VerifyBatch
    let batch1_state = state.batch("batch-000001").unwrap();
    let step1 = plan_resume_step(batch1_state, &recon, &dummy_batch);
    match step1 {
        caravan::resume::ResumeStepPlan::VerifyBatch => {
            // Good, should verify not copy
        }
        caravan::resume::ResumeStepPlan::CopyBatch => {
            panic!("Batch with CopyCompleted phase should plan VerifyBatch, not CopyBatch");
        }
        other => {
            panic!("Unexpected resume step: {:?}", other);
        }
    }

    // Test batch 3 (Planned) -> should plan CopyBatch
    let batch3_state = state.batch("batch-000003").unwrap();
    let step3 = plan_resume_step(batch3_state, &recon, &dummy_batch);
    match step3 {
        caravan::resume::ResumeStepPlan::CopyBatch => {
            // Good, should copy
        }
        other => {
            panic!(
                "Batch with Planned phase should plan CopyBatch, not {:?}",
                other
            );
        }
    }

    // Also test that CopyStarted with ready files -> VerifyBatch
    let copy_started_state = BatchState {
        batch_id: "test".to_string(),
        phase: BatchPhase::CopyStarted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    };
    let step_copy_started = plan_resume_step(&copy_started_state, &recon, &dummy_batch);
    match step_copy_started {
        caravan::resume::ResumeStepPlan::VerifyBatch => {
            // Good
        }
        other => {
            panic!(
                "CopyStarted with ready files should plan VerifyBatch, not {:?}",
                other
            );
        }
    }
}
