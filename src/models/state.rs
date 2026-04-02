use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatchPhase {
    Planned,
    CopyStarted,
    CopyCompleted,
    VerifyCompleted,
    ApprovedForDelete,
    DeleteCompleted,
    SnapshotCompleted,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchState {
    pub batch_id: String,
    pub phase: BatchPhase,
    pub verification_passed: bool,
    pub approved_for_delete: bool,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub event: String,
    pub batch_id: String,
    pub timestamp_unix_secs: u64,
    pub context: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationState {
    pub mode: String,
    pub source: String,
    pub destination: String,
    pub last_successful_snapshot_name: Option<String>,
    pub batches: Vec<BatchState>,
    pub journal: Vec<JournalEntry>,
}

impl MigrationState {
    pub fn new(mode: &str, source: &str, destination: &str) -> Self {
        Self {
            mode: mode.to_string(),
            source: source.to_string(),
            destination: destination.to_string(),
            last_successful_snapshot_name: None,
            batches: Vec::new(),
            journal: Vec::new(),
        }
    }

    pub fn upsert_batch(&mut self, batch: BatchState) {
        if let Some(existing) = self.batches.iter_mut().find(|b| b.batch_id == batch.batch_id) {
            *existing = batch;
        } else {
            self.batches.push(batch);
        }
    }

    pub fn batch(&self, batch_id: &str) -> Option<&BatchState> {
        self.batches.iter().find(|b| b.batch_id == batch_id)
    }
}
