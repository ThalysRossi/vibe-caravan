use crate::error::CaravanError;
use crate::models::state::{BatchPhase, MigrationState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperatorReviewPolicy {
    pub allow_failed_batches: bool,
}

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
        return Err(CaravanError::PolicyBlocked(format!(
            "one or more batches require operator review before continuing: {}",
            failed_batches.join(", ")
        )));
    }

    Ok(())
}

fn ensure_no_failed_verification_batches(state: &MigrationState) -> Result<(), CaravanError> {
    let failed_verification_batches = failed_verification_batches_requiring_review(state);
    if !failed_verification_batches.is_empty() {
        return Err(CaravanError::PolicyBlocked(format!(
            "one or more batches failed verification and require operator review before continuing: {}",
            failed_verification_batches.join(", ")
        )));
    }

    Ok(())
}

pub(crate) fn ensure_no_operator_review_blocks(state: &MigrationState) -> Result<(), CaravanError> {
    ensure_no_operator_review_blocks_with_policy(
        state,
        OperatorReviewPolicy {
            allow_failed_batches: false,
        },
    )
}

pub(crate) fn ensure_no_operator_review_blocks_with_policy(
    state: &MigrationState,
    policy: OperatorReviewPolicy,
) -> Result<(), CaravanError> {
    if !policy.allow_failed_batches {
        ensure_no_failed_batches(state)?;
    }
    ensure_no_failed_verification_batches(state)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_batches(batches: Vec<crate::models::state::BatchState>) -> MigrationState {
        let mut state = MigrationState::new("staging", "/src", "/dst");
        state.batches = batches;
        state
    }

    #[test]
    fn failed_batches_requiring_review_ignores_deleted_entries() {
        let state = state_with_batches(vec![
            crate::models::state::BatchState {
                batch_id: "failed-active".to_string(),
                phase: BatchPhase::Failed,
                verification_passed: false,
                approved_for_delete: false,
                deleted: false,
            },
            crate::models::state::BatchState {
                batch_id: "failed-deleted".to_string(),
                phase: BatchPhase::Failed,
                verification_passed: false,
                approved_for_delete: false,
                deleted: true,
            },
        ]);

        let failed = failed_batches_requiring_review(&state);
        assert_eq!(failed, vec!["failed-active".to_string()]);
    }

    #[test]
    fn failed_verification_batches_requiring_review_filters_by_phase() {
        let state = state_with_batches(vec![
            crate::models::state::BatchState {
                batch_id: "verify-failed".to_string(),
                phase: BatchPhase::VerifyCompleted,
                verification_passed: false,
                approved_for_delete: false,
                deleted: false,
            },
            crate::models::state::BatchState {
                batch_id: "copy-phase".to_string(),
                phase: BatchPhase::CopyCompleted,
                verification_passed: false,
                approved_for_delete: false,
                deleted: false,
            },
        ]);

        let failed = failed_verification_batches_requiring_review(&state);
        assert_eq!(failed, vec!["verify-failed".to_string()]);
    }

    #[test]
    fn operator_review_policy_can_allow_failed_batches_only() {
        let state = state_with_batches(vec![crate::models::state::BatchState {
            batch_id: "failed-active".to_string(),
            phase: BatchPhase::Failed,
            verification_passed: false,
            approved_for_delete: false,
            deleted: false,
        }]);

        ensure_no_operator_review_blocks_with_policy(
            &state,
            OperatorReviewPolicy {
                allow_failed_batches: true,
            },
        )
        .expect("policy should allow failed batches");
    }

    #[test]
    fn failed_verification_blocks_even_when_failed_batches_are_allowed() {
        let state = state_with_batches(vec![crate::models::state::BatchState {
            batch_id: "verify-failed".to_string(),
            phase: BatchPhase::VerifyCompleted,
            verification_passed: false,
            approved_for_delete: false,
            deleted: false,
        }]);

        let err = ensure_no_operator_review_blocks_with_policy(
            &state,
            OperatorReviewPolicy {
                allow_failed_batches: true,
            },
        )
        .expect_err("failed verification must still block");

        assert!(err.to_string().contains("failed verification"));
    }
}
