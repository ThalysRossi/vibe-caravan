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

#[cfg(test)]
mod tests {
    use super::summarize_status;
    use crate::models::state::{BatchPhase, BatchState, JournalEntry, MigrationState};

    #[test]
    fn summarize_status_keeps_core_fields_and_batches() {
        let mut state = MigrationState::new("staging", "/src", "/dst");
        state.snapshot_every = Some(3);
        state.snapshot_dir = Some("/dst/snaps".to_string());
        state.last_successful_snapshot_name = Some("snap-10".to_string());
        state.upsert_batch(BatchState {
            batch_id: "batch-1".to_string(),
            phase: BatchPhase::Planned,
            verification_passed: false,
            approved_for_delete: false,
            deleted: false,
        });

        let summary = summarize_status(&state, 5);

        assert_eq!(summary.mode, "staging");
        assert_eq!(summary.source, "/src");
        assert_eq!(summary.destination, "/dst");
        assert_eq!(summary.snapshot_every, Some(3));
        assert_eq!(summary.snapshot_dir, Some("/dst/snaps"));
        assert_eq!(summary.last_successful_snapshot_name, Some("snap-10"));
        assert_eq!(summary.batches.len(), 1);
        assert_eq!(summary.batches[0].batch_id, "batch-1");
    }

    #[test]
    fn summarize_status_limits_recent_journal_entries_in_reverse_chronological_order() {
        let mut state = MigrationState::new("staging", "/src", "/dst");
        for i in 0..4 {
            state.journal.push(JournalEntry {
                event: format!("event-{i}"),
                batch_id: format!("batch-{i}"),
                timestamp_unix_secs: i,
                context: "ctx".to_string(),
            });
        }

        let summary = summarize_status(&state, 2);

        assert_eq!(summary.journal_entry_count, 4);
        assert_eq!(summary.recent_journal_entries.len(), 2);
        assert_eq!(summary.recent_journal_entries[0].event, "event-3");
        assert_eq!(summary.recent_journal_entries[1].event, "event-2");
    }
}
