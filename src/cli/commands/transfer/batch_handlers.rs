use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use super::super::shared::{
    CopyBatchOp, copy_batch_with_state_updates, ensure_destination_capacity,
    print_copy_batch_banner, print_verification_failed, print_verification_passed,
    print_verify_batch_banner, verify_batch_with_state_updates,
};
use super::context::TransferContext;
use super::setup::planned_batch_state;
use crate::config::ConflictPolicy;
use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::state::{BatchPhase, JournalEntry, MigrationState};

fn effective_conflict_policy(context: &TransferContext<'_>) -> ConflictPolicy {
    if context.config.skip_conflicts {
        ConflictPolicy::SkipFile
    } else {
        context.config.conflict_policy
    }
}

fn mark_batch_failed_for_conflicts(
    batch: &Batch,
    state: &mut MigrationState,
    context: &TransferContext<'_>,
    conflict_report: &crate::conflict::ConflictReport,
    copied_non_conflicting_files: usize,
) -> Result<(), CaravanError> {
    let mut current_batch_state = state
        .batch(&batch.id)
        .cloned()
        .unwrap_or_else(|| planned_batch_state(&batch.id));
    current_batch_state.phase = BatchPhase::Failed;
    current_batch_state.verification_passed = false;
    state.upsert_batch(current_batch_state);
    state.journal.push(JournalEntry {
        event: "copy_failed_conflict".to_string(),
        batch_id: batch.id.clone(),
        timestamp_unix_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        context: format!(
            "naming_conflicts={} size_mismatches={} copied_non_conflicting_files={} skipped_conflicting_files={}",
            conflict_report.total_conflicts,
            conflict_report.size_mismatches.len(),
            copied_non_conflicting_files,
            conflict_report.total_conflicts
        ),
    });
    context.persist_state(state)
}

fn non_conflicting_subset_batch(
    batch: &Batch,
    destination_root: &std::path::Path,
    report: &crate::conflict::ConflictReport,
) -> Batch {
    let conflicting_paths: HashSet<std::path::PathBuf> =
        report.existing_files.iter().cloned().collect();
    let files: Vec<crate::models::file_entry::FileEntry> = batch
        .files
        .iter()
        .filter(|file_entry| {
            let destination_path = destination_root.join(&file_entry.relative_path);
            !conflicting_paths.contains(&destination_path)
        })
        .cloned()
        .collect();
    let total_bytes = files.iter().map(|file_entry| file_entry.size_bytes).sum();

    Batch {
        id: batch.id.clone(),
        file_count: files.len(),
        total_bytes,
        files,
    }
}

pub(super) fn copy_single_batch(
    batch: &Batch,
    context: &TransferContext<'_>,
    state: &mut MigrationState,
) -> Result<(), CaravanError> {
    print_copy_batch_banner(batch);

    let batch_state = state
        .batch(&batch.id)
        .cloned()
        .unwrap_or_else(|| planned_batch_state(&batch.id));

    ensure_destination_capacity(&context.config.dest, batch.total_bytes)?;

    let requires_conflict_check = matches!(
        batch_state.phase,
        BatchPhase::Planned
            | BatchPhase::Failed
            | BatchPhase::CopyCompleted
            | BatchPhase::VerifyCompleted
            | BatchPhase::ApprovedForDelete
            | BatchPhase::DeleteCompleted
            | BatchPhase::SnapshotCompleted
    );
    if requires_conflict_check {
        let conflict_report = crate::conflict::detect_batch_conflicts(batch, &context.config.dest)?;
        if conflict_report.has_conflicts {
            match effective_conflict_policy(context) {
                ConflictPolicy::SkipBatch => {
                    println!(
                        "⚠️  Skipping batch '{}' due to {} naming conflict(s)",
                        batch.id, conflict_report.total_conflicts
                    );
                    mark_batch_failed_for_conflicts(batch, state, context, &conflict_report, 0)?;
                    return Ok(());
                }
                ConflictPolicy::SkipFile => {
                    let copy_subset =
                        non_conflicting_subset_batch(batch, &context.config.dest, &conflict_report);
                    if copy_subset.files.is_empty() {
                        println!(
                            "⚠️  Skipping batch '{}' due to {} naming conflict(s)",
                            batch.id, conflict_report.total_conflicts
                        );
                        mark_batch_failed_for_conflicts(
                            batch,
                            state,
                            context,
                            &conflict_report,
                            0,
                        )?;
                        return Ok(());
                    }

                    println!(
                        "⚠️  Batch '{}' has {} conflicting file(s); copying {} non-conflicting file(s)",
                        batch.id, conflict_report.total_conflicts, copy_subset.file_count
                    );

                    state.upsert_batch(batch_state.clone());
                    let mut persist_state =
                        |current_state: &MigrationState| context.persist_state(current_state);
                    let mut check_shutdown =
                        || crate::signal::check_shutdown(&context.shutdown_flag);
                    copy_batch_with_state_updates(
                        &copy_subset,
                        state,
                        CopyBatchOp {
                            source_root: &context.config.source,
                            dest_root: &context.config.dest,
                            copy_backend: &context.copy_backend,
                            reset_verification_passed: true,
                        },
                        &mut persist_state,
                        &mut check_shutdown,
                        &|batch_id| {
                            CaravanError::StateCorrupt(format!(
                                "batch {} disappeared from state during copy",
                                batch_id
                            ))
                        },
                    )?;

                    mark_batch_failed_for_conflicts(
                        batch,
                        state,
                        context,
                        &conflict_report,
                        copy_subset.file_count,
                    )?;
                    return Ok(());
                }
            }
        }
    }

    state.upsert_batch(batch_state);
    let mut persist_state = |current_state: &MigrationState| context.persist_state(current_state);
    let mut check_shutdown = || crate::signal::check_shutdown(&context.shutdown_flag);
    copy_batch_with_state_updates(
        batch,
        state,
        CopyBatchOp {
            source_root: &context.config.source,
            dest_root: &context.config.dest,
            copy_backend: &context.copy_backend,
            reset_verification_passed: true,
        },
        &mut persist_state,
        &mut check_shutdown,
        &|batch_id| {
            CaravanError::StateCorrupt(format!(
                "batch {} disappeared from state during copy",
                batch_id
            ))
        },
    )?;

    Ok(())
}

pub(super) fn verify_single_batch(
    batch: &Batch,
    context: &TransferContext<'_>,
    state: &mut MigrationState,
) -> Result<(), CaravanError> {
    print_verify_batch_banner(batch);

    let mut persist_state = |current_state: &MigrationState| context.persist_state(current_state);
    let mut check_shutdown = || crate::signal::check_shutdown(&context.shutdown_flag);
    if let Err(err) = verify_batch_with_state_updates(
        batch,
        &context.config.source,
        &context.config.dest,
        state,
        &mut persist_state,
        &mut check_shutdown,
        &|batch_id| {
            CaravanError::StateCorrupt(format!(
                "missing batch state for {} before verification",
                batch_id
            ))
        },
    ) {
        if let CaravanError::VerificationFailed(failure) = &err {
            print_verification_failed(failure);
        }
        return Err(err);
    }

    print_verification_passed();
    Ok(())
}
