use std::path::Path;

use crate::config::OutputFormat;
use crate::error::CaravanError;
use crate::state_store;
use crate::status;

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

    let summary = status::summarize_status(&state, 5);

    print_status_header(
        summary.mode,
        summary.source,
        summary.destination,
        summary.batches.len(),
    );
    print_status_snapshot_policy(summary.snapshot_every, summary.snapshot_dir.map(Path::new));

    for batch in summary.batches {
        print_status_batch(batch);
    }

    if let Some(snapshot) = summary.last_successful_snapshot_name {
        print_last_snapshot(snapshot);
    }

    print_status_journal_header(summary.journal_entry_count);
    for entry in summary.recent_journal_entries {
        print_status_journal_entry(entry);
    }

    Ok(())
}
