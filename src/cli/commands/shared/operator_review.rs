use crate::error::CaravanError;
use crate::models::state::{BatchPhase, MigrationState};

fn failed_batches_requiring_review(state: &MigrationState) -> Vec<String> {
    state
        .batches
        .iter()
        .filter(|batch| batch.phase == BatchPhase::Failed && !batch.deleted)
        .map(|batch| batch.batch_id.clone())
        .collect()
}

fn failed_verification_batches_requiring_review(state: &MigrationState) -> Vec<String> {
    state
        .batches
        .iter()
        .filter(|batch| {
            !batch.deleted
                && !batch.verification_passed
                && matches!(
                    batch.phase,
                    BatchPhase::VerifyCompleted
                        | BatchPhase::ApprovedForDelete
                        | BatchPhase::DeleteCompleted
                        | BatchPhase::SnapshotCompleted
                )
        })
        .map(|batch| batch.batch_id.clone())
        .collect()
}

fn ensure_no_failed_batches(state: &MigrationState) -> Result<(), CaravanError> {
    let failed_batches = failed_batches_requiring_review(state);
    if !failed_batches.is_empty() {
        return Err(CaravanError::InvalidArguments(format!(
            "one or more batches require operator review before continuing: {}",
            failed_batches.join(", ")
        )));
    }

    Ok(())
}

fn ensure_no_failed_verification_batches(state: &MigrationState) -> Result<(), CaravanError> {
    let failed_verification_batches = failed_verification_batches_requiring_review(state);
    if !failed_verification_batches.is_empty() {
        return Err(CaravanError::InvalidArguments(format!(
            "one or more batches failed verification and require operator review before continuing: {}",
            failed_verification_batches.join(", ")
        )));
    }

    Ok(())
}

pub(crate) fn ensure_no_operator_review_blocks(state: &MigrationState) -> Result<(), CaravanError> {
    ensure_no_failed_batches(state)?;
    ensure_no_failed_verification_batches(state)?;
    Ok(())
}
