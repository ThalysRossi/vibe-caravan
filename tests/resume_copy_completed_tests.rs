use std::fs;
use tempfile::TempDir;
use caravan::models::state::{BatchPhase, BatchState, MigrationState, MigrationPhase};
use caravan::state_store::{persist_state, load_state};

#[test]
fn resume_with_copy_completed_batches_should_verify_not_copy() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    
    // Create test files that will be in batches
    fs::write(source_dir.join("file1.txt"), "identical content").expect("create file1");
    fs::write(source_dir.join("file2.txt"), "identical content").expect("create file2");
    
    // Copy files to destination (simulating already copied batches)
    fs::write(dest_dir.join("file1.txt"), "identical content").expect("copy file1");
    fs::write(dest_dir.join("file2.txt"), "identical content").expect("copy file2");
    
    // Create state with 2 batches already CopyCompleted
    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy()
    );
    state.batch_size_bytes = 1024;
    state.migration_phase = MigrationPhase::Copying; // Still in copying phase
    
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
    
    // Save state
    let state_dir = source_dir.join(".caravan");
    fs::create_dir_all(&state_dir).expect("create .caravan");
    let state_path = state_dir.join("state.json");
    persist_state(&state_path, &state).expect("persist state");
    
    // Load state to verify it was saved correctly
    let loaded_state = load_state(&state_path).expect("load state");
    
    // Verify state was saved correctly
    assert_eq!(loaded_state.batches.len(), 2);
    assert_eq!(loaded_state.batches[0].batch_id, "batch-000001");
    assert_eq!(loaded_state.batches[0].phase, BatchPhase::CopyCompleted);
    assert!(!loaded_state.batches[0].verification_passed);
    assert_eq!(loaded_state.batches[1].batch_id, "batch-000002");
    assert_eq!(loaded_state.batches[1].phase, BatchPhase::CopyCompleted);
    assert!(!loaded_state.batches[1].verification_passed);
    
    // Now test the resume planning logic
    use caravan::resume::{plan_resume_step, ReconciliationResult};
    use caravan::models::batch::Batch;
    
    let recon = ReconciliationResult {
        all_destination_files_ready: true,
        missing_in_destination: vec![],
        size_mismatches: vec![],
    };
    
    let dummy_batch = Batch {
        id: "batch-000001".to_string(),
        files: vec![],
        total_bytes: 0,
        file_count: 0,
    };
    
    let batch1_state = loaded_state.batch("batch-000001").unwrap();
    let step = plan_resume_step(batch1_state, &recon, &dummy_batch);
    
    // Should plan verification, not copying
    match step {
        caravan::resume::ResumeStepPlan::VerifyBatch => {
            // Correct!
        }
        caravan::resume::ResumeStepPlan::CopyBatch => {
            panic!("CopyCompleted batch with ready files should plan VerifyBatch, not CopyBatch");
        }
        other => {
            panic!("Unexpected resume step for CopyCompleted batch: {:?}", other);
        }
    }
}

#[test]
fn resume_with_copy_completed_but_missing_files_should_require_review() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    
    // Create state with CopyCompleted batch
    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy()
    );
    state.batch_size_bytes = 1024;
    state.migration_phase = MigrationPhase::Copying;
    
    state.batches.push(BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::CopyCompleted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });
    
    // Test resume planning with missing files
    use caravan::resume::{plan_resume_step, ReconciliationResult};
    use caravan::models::batch::Batch;
    
    let recon = ReconciliationResult {
        all_destination_files_ready: false,
        missing_in_destination: vec!["file1.txt".to_string()],
        size_mismatches: vec![],
    };
    
    let dummy_batch = Batch {
        id: "batch-000001".to_string(),
        files: vec![],
        total_bytes: 0,
        file_count: 0,
    };
    
    let batch_state = state.batch("batch-000001").unwrap();
    let step = plan_resume_step(batch_state, &recon, &dummy_batch);
    
    // Should require operator review when state says copied but files are missing
    match step {
        caravan::resume::ResumeStepPlan::ConflictOperatorReview { reason: _ } => {
            // Correct - inconsistency between state and filesystem
        }
        other => {
            panic!("CopyCompleted batch with missing files should require operator review, not {:?}", other);
        }
    }
}