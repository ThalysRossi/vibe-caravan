use crate::models::state::MigrationState;
use crate::plan::PlanningSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferPlanningSummary {
    pub batch_count: usize,
    pub source_file_count: usize,
    pub source_total_bytes: u64,
}

pub fn summarize_transfer_plan(plan: &PlanningSnapshot) -> TransferPlanningSummary {
    TransferPlanningSummary {
        batch_count: plan.batches.len(),
        source_file_count: plan.source_file_count,
        source_total_bytes: plan.source_total_bytes,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferExecutionSummary {
    pub processed_batches: u32,
    pub total_batches: usize,
    pub completed_batches: usize,
    pub pending_delete_batches: usize,
}

pub fn summarize_transfer_execution(
    state: &MigrationState,
    processed_batches: u32,
) -> TransferExecutionSummary {
    let total_batches = state.batches.len();
    let completed_batches = state.batches.iter().filter(|b| b.deleted).count();
    let pending_delete_batches = state.batches.iter().filter(|b| !b.deleted).count();

    TransferExecutionSummary {
        processed_batches,
        total_batches,
        completed_batches,
        pending_delete_batches,
    }
}
