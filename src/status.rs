use crate::models::state::{BatchState, JournalEntry, MigrationState};

pub struct StatusSummary<'a> {
    pub mode: &'a str,
    pub source: &'a str,
    pub destination: &'a str,
    pub snapshot_every: Option<u32>,
    pub snapshot_dir: Option<&'a str>,
    pub batches: &'a [BatchState],
    pub last_successful_snapshot_name: Option<&'a str>,
    pub journal_entry_count: usize,
    pub recent_journal_entries: Vec<&'a JournalEntry>,
}

pub fn summarize_status(state: &MigrationState, journal_limit: usize) -> StatusSummary<'_> {
    let recent_journal_entries = state.journal.iter().rev().take(journal_limit).collect();

    StatusSummary {
        mode: &state.mode,
        source: &state.source,
        destination: &state.destination,
        snapshot_every: state.snapshot_every,
        snapshot_dir: state.snapshot_dir.as_deref(),
        batches: &state.batches,
        last_successful_snapshot_name: state.last_successful_snapshot_name.as_deref(),
        journal_entry_count: state.journal.len(),
        recent_journal_entries,
    }
}
