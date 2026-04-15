use std::path::Path;

use crate::error::CaravanError;
use crate::state_store;

pub(super) fn execute_status(state_path: &Path) -> Result<(), CaravanError> {
    let state = state_store::load_state(state_path)?;

    println!("=== Caravan Status ===");
    println!("Mode: {}", state.mode);
    println!("Source: {}", state.source);
    println!("Destination: {}", state.destination);
    println!("Batches: {}", state.batches.len());

    for batch in &state.batches {
        println!(
            "  {} - {:?} (verified: {}, approved: {}, deleted: {})",
            batch.batch_id,
            batch.phase,
            batch.verification_passed,
            batch.approved_for_delete,
            batch.deleted
        );
    }

    if let Some(snapshot) = &state.last_successful_snapshot_name {
        println!("Last snapshot: {}", snapshot);
    }

    println!("\nJournal entries: {}", state.journal.len());
    for entry in state.journal.iter().rev().take(5) {
        println!(
            "  [{}] {} - {} ({})",
            entry.timestamp_unix_secs, entry.event, entry.batch_id, entry.context
        );
    }

    Ok(())
}
