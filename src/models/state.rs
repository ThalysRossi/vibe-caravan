use crate::config::CopyStrategy;
use crate::models::batch::Batch;
use crate::models::file_entry::FileEntry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    #[serde(default)]
    pub planned_batches: Vec<PlannedBatch>,
    pub migration_phase: MigrationPhase,
    pub last_successful_snapshot_name: Option<String>,
    pub batches: Vec<BatchState>,
    pub journal: Vec<JournalEntry>,
    #[serde(skip)]
    lookup_index: MigrationStateLookupIndex,
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
            planned_batches: Vec::new(),
            migration_phase: MigrationPhase::NotStarted,
            last_successful_snapshot_name: None,
            batches: Vec::new(),
            journal: Vec::new(),
            lookup_index: MigrationStateLookupIndex::default(),
        }
    }

    pub fn rebuild_indexes(&mut self) {
        self.lookup_index
            .rebuild(&self.batches, &self.planned_batches);
    }

    fn ensure_batch_index_consistent(&mut self) {
        if !self.lookup_index.is_batch_index_consistent(&self.batches) {
            self.lookup_index.rebuild_batch_index(&self.batches);
        }
    }

    fn ensure_planned_batch_index_consistent(&mut self) {
        if !self
            .lookup_index
            .is_planned_batch_index_consistent(&self.planned_batches)
        {
            self.lookup_index
                .rebuild_planned_batch_index(&self.planned_batches);
        }
    }

    fn batch_index_lookup(&self, batch_id: &str) -> Option<usize> {
        let index = *self.lookup_index.batch_pos_by_id.get(batch_id)?;
        match self.batches.get(index) {
            Some(batch) if batch.batch_id == batch_id => Some(index),
            _ => None,
        }
    }

    fn planned_batch_index_lookup(&self, batch_id: &str) -> Option<usize> {
        let index = *self.lookup_index.planned_batch_pos_by_id.get(batch_id)?;
        match self.planned_batches.get(index) {
            Some(batch) if batch.batch_id == batch_id => Some(index),
            _ => None,
        }
    }

    pub fn upsert_batch(&mut self, batch: BatchState) {
        self.ensure_batch_index_consistent();
        let batch_id = batch.batch_id.clone();
        if let Some(index) = self.lookup_index.batch_pos_by_id.get(&batch_id).copied() {
            self.batches[index] = batch;
        } else {
            self.batches.push(batch);
            let index = self.batches.len() - 1;
            self.lookup_index
                .batch_pos_by_id
                .insert(batch_id.clone(), index);
            self.lookup_index.batch_ids_in_order.push(batch_id);
        }
    }

    pub fn batch(&self, batch_id: &str) -> Option<&BatchState> {
        if let Some(index) = self.batch_index_lookup(batch_id) {
            return self.batches.get(index);
        }
        self.batches.iter().find(|b| b.batch_id == batch_id)
    }

    pub fn batch_mut(&mut self, batch_id: &str) -> Option<&mut BatchState> {
        self.ensure_batch_index_consistent();
        if let Some(index) = self.lookup_index.batch_pos_by_id.get(batch_id).copied() {
            return self.batches.get_mut(index);
        }
        self.batches.iter_mut().find(|b| b.batch_id == batch_id)
    }

    pub fn upsert_planned_batch(&mut self, planned_batch: PlannedBatch) {
        self.ensure_planned_batch_index_consistent();
        let batch_id = planned_batch.batch_id.clone();
        if let Some(index) = self
            .lookup_index
            .planned_batch_pos_by_id
            .get(&batch_id)
            .copied()
        {
            self.planned_batches[index] = planned_batch;
        } else {
            self.planned_batches.push(planned_batch);
            let index = self.planned_batches.len() - 1;
            self.lookup_index
                .planned_batch_pos_by_id
                .insert(batch_id.clone(), index);
            self.lookup_index.planned_batch_ids_in_order.push(batch_id);
        }
    }

    pub fn planned_batch(&self, batch_id: &str) -> Option<&PlannedBatch> {
        if let Some(index) = self.planned_batch_index_lookup(batch_id) {
            return self.planned_batches.get(index);
        }
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
        self.ensure_batch_index_consistent();
        for batch_id in batch_ids {
            if let Some(index) = self.lookup_index.batch_pos_by_id.get(batch_id).copied() {
                let batch = &mut self.batches[index];
                batch.approved_for_delete = true;
                batch.phase = BatchPhase::ApprovedForDelete;
            }
        }
    }
}

const fn default_copy_strategy() -> CopyStrategy {
    CopyStrategy::Auto
}

#[derive(Debug, Clone, Default)]
struct MigrationStateLookupIndex {
    batch_pos_by_id: HashMap<String, usize>,
    planned_batch_pos_by_id: HashMap<String, usize>,
    batch_ids_in_order: Vec<String>,
    planned_batch_ids_in_order: Vec<String>,
}

impl MigrationStateLookupIndex {
    fn rebuild(&mut self, batches: &[BatchState], planned_batches: &[PlannedBatch]) {
        self.rebuild_batch_index(batches);
        self.rebuild_planned_batch_index(planned_batches);
    }

    fn rebuild_batch_index(&mut self, batches: &[BatchState]) {
        self.batch_pos_by_id.clear();
        self.batch_ids_in_order.clear();
        self.batch_ids_in_order.reserve(batches.len());

        for (index, batch) in batches.iter().enumerate() {
            self.batch_pos_by_id.insert(batch.batch_id.clone(), index);
            self.batch_ids_in_order.push(batch.batch_id.clone());
        }
    }

    fn rebuild_planned_batch_index(&mut self, planned_batches: &[PlannedBatch]) {
        self.planned_batch_pos_by_id.clear();
        self.planned_batch_ids_in_order.clear();
        self.planned_batch_ids_in_order
            .reserve(planned_batches.len());

        for (index, planned_batch) in planned_batches.iter().enumerate() {
            self.planned_batch_pos_by_id
                .insert(planned_batch.batch_id.clone(), index);
            self.planned_batch_ids_in_order
                .push(planned_batch.batch_id.clone());
        }
    }

    fn is_batch_index_consistent(&self, batches: &[BatchState]) -> bool {
        if self.batch_pos_by_id.len() != batches.len()
            || self.batch_ids_in_order.len() != batches.len()
        {
            return false;
        }

        for (index, batch) in batches.iter().enumerate() {
            if self.batch_ids_in_order.get(index).map(String::as_str)
                != Some(batch.batch_id.as_str())
            {
                return false;
            }
            if self.batch_pos_by_id.get(&batch.batch_id).copied() != Some(index) {
                return false;
            }
        }

        true
    }

    fn is_planned_batch_index_consistent(&self, planned_batches: &[PlannedBatch]) -> bool {
        if self.planned_batch_pos_by_id.len() != planned_batches.len()
            || self.planned_batch_ids_in_order.len() != planned_batches.len()
        {
            return false;
        }

        for (index, planned_batch) in planned_batches.iter().enumerate() {
            if self
                .planned_batch_ids_in_order
                .get(index)
                .map(String::as_str)
                != Some(planned_batch.batch_id.as_str())
            {
                return false;
            }
            if self
                .planned_batch_pos_by_id
                .get(&planned_batch.batch_id)
                .copied()
                != Some(index)
            {
                return false;
            }
        }

        true
    }
}

impl PartialEq for MigrationStateLookupIndex {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for MigrationStateLookupIndex {}
