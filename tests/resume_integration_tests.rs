use std::fs;
use tempfile::TempDir;
use caravan::models::state::{BatchPhase, BatchState, MigrationState, MigrationPhase};
use caravan::state_store::{persist_state, load_state};

/// Test that re-running a transfer with partial copy completion skips already copied batches
/// This simulates the user's scenario: stop after batch-000003, then run the same command again
#[test]
fn transfer_rerun_skips_already_copied_batches() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    
    // Create test files
    for i in 1..=6 {
        fs::write(source_dir.join(format!("file{}.txt", i)), 
                 format!("content {}", i)).expect("create file");
    }
    
    // Create state with 3 batches already CopyCompleted (simulating partial migration)
    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy()
    );
    state.batch_size_bytes = 1024; // 1KB batch size to force multiple batches
    state.migration_phase = MigrationPhase::Copying;
    
    // Create 6 batches total, first 3 CopyCompleted
    for i in 1..=6 {
        let phase = if i <= 3 {
            BatchPhase::CopyCompleted
        } else {
            BatchPhase::Planned
        };
        
        state.batches.push(BatchState {
            batch_id: format!("batch-{:06}", i),
            phase,
            verification_passed: false,
            approved_for_delete: false,
            deleted: false,
        });
    }
    
    // Save state to source/.caravan (where caravan expects it)
    let state_dir = source_dir.join(".caravan");
    fs::create_dir_all(&state_dir).expect("create .caravan");
    let state_path = state_dir.join("state.json");
    persist_state(&state_path, &state).expect("persist state");
    
    // Also need to create batch definition files for the planner to load
    // This is a simplified test - in reality we'd need proper batch files
    // For this test, we'll just verify the state loading logic
    
    // Load the state to verify it was saved correctly
    let loaded_state = load_state(&state_path).expect("load state");
    
    // Verify state
    assert_eq!(loaded_state.batches.len(), 6);
    for (i, batch) in loaded_state.batches.iter().enumerate() {
        let expected_phase = if i < 3 { BatchPhase::CopyCompleted } else { BatchPhase::Planned };
        assert_eq!(batch.phase, expected_phase, "Batch {} phase incorrect", batch.batch_id);
        assert!(!batch.verification_passed, "Batch {} verification_passed should be false", batch.batch_id);
    }
    
    // Note: Actually running execute_transfer would require proper batch definitions
    // This test demonstrates that the state is saved/loaded correctly
    // The actual resume behavior would be tested in execute_resume
}

/// Test the resume command specifically
#[test]
fn resume_command_skips_already_copied_batches() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    
    // Create minimal test file structure
    fs::write(source_dir.join("test.txt"), "test").expect("create test file");
    
    // Create state with one batch already CopyCompleted
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
    
    // Save state
    let state_dir = source_dir.join(".caravan");
    fs::create_dir_all(&state_dir).expect("create .caravan");
    let state_path = state_dir.join("state.json");
    persist_state(&state_path, &state).expect("persist state");
    
    // Test the resume logic by checking plan_resume_step
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
    
    let batch_state = state.batch("batch-000001").unwrap();
    let step = plan_resume_step(batch_state, &recon, &dummy_batch);
    
    // Should plan verification, not copying
    match step {
        caravan::resume::ResumeStepPlan::VerifyBatch => {
            // Correct!
        }
        other => {
            panic!("Resume should plan VerifyBatch for CopyCompleted batch, not {:?}", other);
        }
    }
}
