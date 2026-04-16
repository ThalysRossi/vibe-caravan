use crate::config::{CopyStrategy, TransferConfig};
use crate::models::batch::Batch;
use crate::models::file_entry::FileEntry;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
pub struct PlannedFile {
    pub relative_path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedBatch {
    pub batch_id: String,
    pub file_count: usize,
    pub total_bytes: u64,
    pub files: Vec<PlannedFile>,
}

impl PlannedBatch {
    pub fn from_batch(batch: &Batch) -> Self {
        Self {
            batch_id: batch.id.clone(),
            file_count: batch.file_count,
            total_bytes: batch.total_bytes,
            files: batch
                .files
                .iter()
                .map(|file| PlannedFile {
                    relative_path: file.relative_path.clone(),
                    size_bytes: file.size_bytes,
                })
                .collect(),
        }
    }

    pub fn to_batch(&self) -> Batch {
        Batch {
            id: self.batch_id.clone(),
            file_count: self.file_count,
            total_bytes: self.total_bytes,
            files: self
                .files
                .iter()
                .map(|file| FileEntry {
                    relative_path: file.relative_path.clone(),
                    size_bytes: file.size_bytes,
                    modified_time: None,
                })
                .collect(),
        }
    }
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
    #[serde(default)]
    pub snapshot_dir: Option<String>,
    #[serde(default = "default_copy_strategy")]
    pub copy_strategy: CopyStrategy,
    #[serde(default = "TransferConfig::default_copy_buffer_size")]
    pub copy_buffer_size: usize,
    #[serde(default = "TransferConfig::default_buffered_copy_threshold")]
    pub buffered_copy_threshold: u64,
    #[serde(default)]
    pub planned_batches: Vec<PlannedBatch>,
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
            snapshot_dir: None,
            copy_strategy: default_copy_strategy(),
            copy_buffer_size: TransferConfig::default_copy_buffer_size(),
            buffered_copy_threshold: TransferConfig::default_buffered_copy_threshold(),
            planned_batches: Vec::new(),
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

    pub fn upsert_planned_batch(&mut self, planned_batch: PlannedBatch) {
        if let Some(existing) = self
            .planned_batches
            .iter_mut()
            .find(|batch| batch.batch_id == planned_batch.batch_id)
        {
            *existing = planned_batch;
        } else {
            self.planned_batches.push(planned_batch);
        }
    }

    pub fn planned_batch(&self, batch_id: &str) -> Option<&PlannedBatch> {
        self.planned_batches
            .iter()
            .find(|batch| batch.batch_id == batch_id)
    }

    pub fn materialize_planned_batch(&self, batch_id: &str) -> Option<Batch> {
        self.planned_batch(batch_id).map(PlannedBatch::to_batch)
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

const fn default_copy_strategy() -> CopyStrategy {
    CopyStrategy::Auto
}
