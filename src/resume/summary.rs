use crate::models::state::MigrationState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResumeStateSummary<'a> {
    pub mode: &'a str,
    pub source: &'a str,
    pub destination: &'a str,
    pub total_batches: usize,
    pub completed_batches: usize,
}

pub fn summarize_resume_state(state: &MigrationState) -> ResumeStateSummary<'_> {
    ResumeStateSummary {
        mode: &state.mode,
        source: &state.source,
        destination: &state.destination,
        total_batches: state.batches.len(),
        completed_batches: state.batches.iter().filter(|batch| batch.deleted).count(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResumeExecutionSummary {
    pub total_batches: usize,
    pub completed_batches: usize,
    pub pending_delete_batches: usize,
}

pub fn summarize_resume_execution(state: &MigrationState) -> ResumeExecutionSummary {
    ResumeExecutionSummary {
        total_batches: state.batches.len(),
        completed_batches: state.batches.iter().filter(|batch| batch.deleted).count(),
        pending_delete_batches: state.batches.iter().filter(|batch| !batch.deleted).count(),
    }
}
