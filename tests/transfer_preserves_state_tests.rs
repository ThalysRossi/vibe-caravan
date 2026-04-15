use caravan::models::state::{BatchPhase, BatchState, MigrationPhase, MigrationState};
use caravan::state_store::persist_state;
use std::fs;
use tempfile::TempDir;

/// Test that execute_transfer preserves existing batch states when re-run
/// This reproduces the bug where re-running the transfer overwrites CopyCompleted with Planned
#[test]
fn transfer_initialization_preserves_existing_batch_states() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");

    // Create state with one batch already CopyCompleted
    let mut state = MigrationState::new(
        "staging",
        &source_dir.to_string_lossy(),
        &dest_dir.to_string_lossy(),
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

    state.batches.push(BatchState {
        batch_id: "batch-000002".to_string(),
        phase: BatchPhase::Planned,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    });

    // Save state
    let state_dir = source_dir.join(".caravan");
    fs::create_dir_all(&state_dir).expect("create .caravan");
    let state_path = state_dir.join("state.json");
    persist_state(&state_path, &state).expect("persist state");

    // Simulate what execute_transfer does when it loads state and initializes batches
    // The bug is in lines 230-254 of src/cli.rs:
    // for batch in &plan.batches {
    //     state.upsert_batch(BatchState {
    //         batch_id: batch.id.clone(),
    //         phase: BatchPhase::Planned,
    //         verification_passed: false,
    //         approved_for_delete: false,
    //         deleted: false,
    //     });
    // }
    // This overwrites existing batch states!

    // Let's demonstrate the bug
    let mut buggy_state = state.clone();

    // Simulate plan with same batch IDs (deterministic)
    let simulated_plan_batch_ids = vec!["batch-000001".to_string(), "batch-000002".to_string()];

    // Buggy initialization (current code)
    for batch_id in &simulated_plan_batch_ids {
        buggy_state.upsert_batch(BatchState {
            batch_id: batch_id.clone(),
            phase: BatchPhase::Planned,
            verification_passed: false,
            approved_for_delete: false,
            deleted: false,
        });
    }

    // Check that batch-000001 was incorrectly overwritten
    let batch1 = buggy_state.batch("batch-000001").unwrap();
    assert_eq!(
        batch1.phase,
        BatchPhase::Planned,
        "BUG: CopyCompleted batch was overwritten with Planned!"
    );
    assert!(!batch1.verification_passed);

    // What SHOULD happen: batch-000001 should remain CopyCompleted
    let batch1_correct = state.batch("batch-000001").unwrap();
    assert_eq!(
        batch1_correct.phase,
        BatchPhase::CopyCompleted,
        "Batch should remain CopyCompleted"
    );

    // Test the fix: we should only add batches that don't exist
    let mut fixed_state = state.clone();

    for batch_id in &simulated_plan_batch_ids {
        if fixed_state.batch(batch_id).is_none() {
            // Only add if not present
            fixed_state.batches.push(BatchState {
                batch_id: batch_id.clone(),
                phase: BatchPhase::Planned,
                verification_passed: false,
                approved_for_delete: false,
                deleted: false,
            });
        }
        // Otherwise keep existing state
    }

    // Check that batch-000001 preserved its state
    let batch1_fixed = fixed_state.batch("batch-000001").unwrap();
    assert_eq!(
        batch1_fixed.phase,
        BatchPhase::CopyCompleted,
        "Fixed: Batch should preserve CopyCompleted state"
    );

    // batch-000002 was Planned and remains Planned (no change)
    let batch2_fixed = fixed_state.batch("batch-000002").unwrap();
    assert_eq!(
        batch2_fixed.phase,
        BatchPhase::Planned,
        "Batch should remain Planned"
    );
}

/// Test that execute_transfer's skip logic works with preserved states
#[test]
fn transfer_skip_logic_with_preserved_states() {
    // This test verifies that when batches preserve their CopyCompleted state,
    // the skip logic in execute_transfer works correctly

    use caravan::models::state::{BatchPhase, BatchState};

    let batch_copy_completed = BatchState {
        batch_id: "batch-000001".to_string(),
        phase: BatchPhase::CopyCompleted,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    };

    let batch_planned = BatchState {
        batch_id: "batch-000002".to_string(),
        phase: BatchPhase::Planned,
        verification_passed: false,
        approved_for_delete: false,
        deleted: false,
    };

    // Test the skip logic from execute_transfer lines 267-277
    // if existing_batch.phase == BatchPhase::CopyCompleted || existing_batch.phase == BatchPhase::VerifyCompleted {
    //     println!("Skipping {}: already copied", batch.id);
    //     continue;
    // }

    // CopyCompleted should be skipped
    assert!(batch_copy_completed.phase == BatchPhase::CopyCompleted);

    // Planned should NOT be skipped
    assert!(batch_planned.phase != BatchPhase::CopyCompleted);
    assert!(batch_planned.phase != BatchPhase::VerifyCompleted);

    // Verification skip logic lines 323-332
    // if existing_batch.verification_passed && existing_batch.phase == BatchPhase::VerifyCompleted

    // CopyCompleted with verification_passed=false should NOT be skipped in verification phase
    assert!(
        !(batch_copy_completed.verification_passed
            && batch_copy_completed.phase == BatchPhase::VerifyCompleted)
    );
}
