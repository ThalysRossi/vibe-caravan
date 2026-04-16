use std::path::Path;

use crate::config::OutputFormat;
use crate::error::CaravanError;
use crate::state_store;

use super::shared::{
    print_last_snapshot, print_status_batch, print_status_header, print_status_journal_entry,
    print_status_journal_header, print_status_snapshot_policy,
};

pub(super) fn execute_status(state_path: &Path, output: OutputFormat) -> Result<(), CaravanError> {
    let state = state_store::load_state(state_path)?;

    if output == OutputFormat::Json {
        let serialized = serde_json::to_string_pretty(&state)
            .map_err(|err| CaravanError::Io(format!("failed to serialize status output: {err}")))?;
        println!("{serialized}");
        return Ok(());
    }

    print_status_header(
        &state.mode,
        &state.source,
        &state.destination,
        state.batches.len(),
    );
    print_status_snapshot_policy(
        state.snapshot_every,
        state.snapshot_dir.as_deref().map(Path::new),
    );

    for batch in &state.batches {
        print_status_batch(batch);
    }

    if let Some(snapshot) = &state.last_successful_snapshot_name {
        print_last_snapshot(snapshot);
    }

    print_status_journal_header(state.journal.len());
    for entry in state.journal.iter().rev().take(5) {
        print_status_journal_entry(entry);
    }

    Ok(())
}
