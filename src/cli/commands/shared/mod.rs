mod app_context;
mod batch_ops;
mod capacity_guard;
mod deletion;
mod operator_review;
mod output;

pub(super) use app_context::AppContext;
pub(super) use batch_ops::{
    CopyBatchOp, copy_batch_with_state_updates, handle_verification_error,
    mark_batch_failed_for_conflicts, non_conflicting_subset_batch, verify_batch_with_state_updates,
};
pub(super) use capacity_guard::ensure_destination_capacity_for_batch;
pub(super) use deletion::approve_and_delete_verified_batches;
pub(super) use operator_review::{
    OperatorReviewPolicy, ensure_no_operator_review_blocks,
    ensure_no_operator_review_blocks_with_policy,
};
pub(super) use output::{
    print_all_verified_banner, print_copy_batch_banner, print_delete_source_batches_banner,
    print_deletion_not_approved, print_last_snapshot, print_migration_complete, print_phase_banner,
    print_plan_summary, print_preflight_warnings, print_resume_complete,
    print_resume_completed_batches, print_resume_continue_to_deletion,
    print_resume_processing_batch_banner, print_resume_skip_already_completed,
    print_resume_state_details, print_resume_state_header, print_resume_verification_passed,
    print_resuming_transfer, print_skip_already_completed, print_skip_copy_already_completed,
    print_skip_verification_already_completed, print_skip_verification_requires_operator_review,
    print_state_save_locations, print_status_batch, print_status_header,
    print_status_journal_entry, print_status_journal_header, print_status_snapshot_policy,
    print_verification_passed, print_verify_batch_banner,
};
