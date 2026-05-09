use crate::models::batch::Batch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanOptions {
    pub batch_size_bytes: u64,
    pub max_files: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningSnapshot {
    pub source_file_count: usize,
    pub source_total_bytes: u64,
    pub batches: Vec<Batch>,
}
