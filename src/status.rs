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
    use super::*;

    use crate::models::state::{BatchPhase, MigrationPhase};

    #[test]
    fn summarize_status_returns_core_fields_and_recent_journal_entries() {
        let mut state = MigrationState::new("migrate", "/src", "/dst");
        state.migration_phase = MigrationPhase::Copying;
        state.snapshot_every = Some(3);
        state.snapshot_dir = Some("/snapshots".to_string());
        state.last_successful_snapshot_name = Some("snap-3".to_string());
        state.batches.push(crate::models::state::BatchState {
            batch_id: "batch-1".to_string(),
            phase: BatchPhase::VerifyCompleted,
            verification_passed: true,
            approved_for_delete: true,
            deleted: false,
        });
        state.journal.push(crate::models::state::JournalEntry {
            event: "copy_completed".to_string(),
            batch_id: "batch-1".to_string(),
            timestamp_unix_secs: 1,
            context: "ctx-1".to_string(),
        });
        state.journal.push(crate::models::state::JournalEntry {
            event: "verify_completed".to_string(),
            batch_id: "batch-1".to_string(),
            timestamp_unix_secs: 2,
            context: "ctx-2".to_string(),
        });
        state.journal.push(crate::models::state::JournalEntry {
            event: "delete_completed".to_string(),
            batch_id: "batch-1".to_string(),
            timestamp_unix_secs: 3,
            context: "ctx-3".to_string(),
        });

        let summary = summarize_status(&state, 2);

        assert_eq!(summary.mode, "migrate");
        assert_eq!(summary.source, "/src");
        assert_eq!(summary.destination, "/dst");
        assert_eq!(summary.snapshot_every, Some(3));
        assert_eq!(summary.snapshot_dir, Some("/snapshots"));
        assert_eq!(summary.last_successful_snapshot_name, Some("snap-3"));
        assert_eq!(summary.batches.len(), 1);
        assert_eq!(summary.journal_entry_count, 3);
        assert_eq!(summary.recent_journal_entries.len(), 2);
        assert_eq!(summary.recent_journal_entries[0].event, "delete_completed");
        assert_eq!(summary.recent_journal_entries[1].event, "verify_completed");
    }

    #[test]
    fn summarize_status_handles_zero_journal_limit() {
        let mut state = MigrationState::new("staging", "/src", "/dst");
        state.journal.push(crate::models::state::JournalEntry {
            event: "planned".to_string(),
            batch_id: "batch-1".to_string(),
            timestamp_unix_secs: 1,
            context: "ctx".to_string(),
        });

        let summary = summarize_status(&state, 0);
        assert_eq!(summary.journal_entry_count, 1);
        assert!(summary.recent_journal_entries.is_empty());
    }
}
