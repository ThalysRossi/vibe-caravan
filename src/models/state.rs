use crate::config::{CopyStrategy, TransferConfig, VerificationMode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationPhase {
    NotStarted,
    Copying,
    Verifying,
    AwaitingDeletion,
    Completed,
    Failed,
}

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
    pub batch_size_bytes: u64,
    #[serde(default)]
    pub max_files: Option<u64>,
    #[serde(default)]
    pub snapshot_every: Option<u32>,
    #[serde(default = "default_verification_mode")]
    pub verification_mode: VerificationMode,
    #[serde(default = "default_copy_strategy")]
    pub copy_strategy: CopyStrategy,
    #[serde(default = "TransferConfig::default_copy_buffer_size")]
    pub copy_buffer_size: usize,
    #[serde(default = "TransferConfig::default_buffered_copy_threshold")]
    pub buffered_copy_threshold: u64,
    pub migration_phase: MigrationPhase,
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
            batch_size_bytes: 0,
            max_files: None,
            snapshot_every: None,
            verification_mode: default_verification_mode(),
            copy_strategy: default_copy_strategy(),
            copy_buffer_size: TransferConfig::default_copy_buffer_size(),
            buffered_copy_threshold: TransferConfig::default_buffered_copy_threshold(),
            migration_phase: MigrationPhase::NotStarted,
            last_successful_snapshot_name: None,
            batches: Vec::new(),
            journal: Vec::new(),
        }
    }

    pub fn upsert_batch(&mut self, batch: BatchState) {
        if let Some(existing) = self
            .batches
            .iter_mut()
            .find(|b| b.batch_id == batch.batch_id)
        {
            *existing = batch;
        } else {
            self.batches.push(batch);
        }
    }

    pub fn batch(&self, batch_id: &str) -> Option<&BatchState> {
        self.batches.iter().find(|b| b.batch_id == batch_id)
    }

    pub fn batch_mut(&mut self, batch_id: &str) -> Option<&mut BatchState> {
        self.batches.iter_mut().find(|b| b.batch_id == batch_id)
    }

    pub fn batches_needing_approval(&self) -> Vec<String> {
        self.batches
            .iter()
            .filter(|b| b.verification_passed && !b.approved_for_delete && !b.deleted)
            .map(|b| b.batch_id.clone())
            .collect()
    }

    pub fn batches_approved_but_not_deleted(&self) -> Vec<String> {
        self.batches
            .iter()
            .filter(|b| b.approved_for_delete && !b.deleted)
            .map(|b| b.batch_id.clone())
            .collect()
    }

    pub fn batches_verified_and_ready(&self) -> Vec<String> {
        self.batches
            .iter()
            .filter(|b| {
                b.phase == BatchPhase::VerifyCompleted
                    && b.verification_passed
                    && !b.approved_for_delete
                    && !b.deleted
            })
            .map(|b| b.batch_id.clone())
            .collect()
    }

    pub fn approve_batches(&mut self, batch_ids: &[String]) {
        for batch_id in batch_ids {
            if let Some(batch) = self.batch_mut(batch_id) {
                batch.approved_for_delete = true;
                batch.phase = BatchPhase::ApprovedForDelete;
            }
        }
    }
}

const fn default_verification_mode() -> VerificationMode {
    VerificationMode::Digest
}

const fn default_copy_strategy() -> CopyStrategy {
    CopyStrategy::Auto
}
