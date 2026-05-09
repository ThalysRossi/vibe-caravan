use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::ConflictPolicy;
use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::state::{BatchPhase, JournalEntry, MigrationState};
use crate::resume as resume_ops;

use super::super::shared::{
    CopyBatchOp, copy_batch_with_state_updates, ensure_destination_capacity,
    print_resume_continue_to_deletion, print_resume_processing_batch_banner,
    print_resume_skip_already_completed, print_resume_verification_passed,
    print_verification_failed, verify_batch_with_state_updates,
};
use super::context::ResumeContext;

fn effective_conflict_policy(context: &ResumeContext<'_>) -> ConflictPolicy {
    if context.config.recover_failed {
        ConflictPolicy::SkipFile
    } else {
        context.config.conflict_policy
    }
}

fn mark_batch_failed_for_conflicts(
    batch: &Batch,
    state: &mut MigrationState,
    context: &ResumeContext<'_>,
    conflict_report: &crate::conflict::ConflictReport,
    copied_non_conflicting_files: usize,
) -> Result<(), CaravanError> {
    let mut current_batch_state = state.batch(&batch.id).cloned().ok_or_else(|| {
        CaravanError::StateCorrupt(format!(
            "batch {} disappeared from state during conflict handling",
            batch.id
        ))
    })?;
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
    let files = batch
        .files
        .iter()
        .filter(|file_entry| {
            let destination_path = destination_root.join(&file_entry.relative_path);
            !conflicting_paths.contains(&destination_path)
        })
        .cloned()
        .collect::<Vec<_>>();
    let total_bytes = files.iter().map(|file_entry| file_entry.size_bytes).sum();

    Batch {
        id: batch.id.clone(),
        file_count: files.len(),
        total_bytes,
        files,
    }
}

fn verify_batch_for_resume(
    batch: &Batch,
    context: &ResumeContext<'_>,
    state: &mut MigrationState,
) -> Result<(), CaravanError> {
    println!("Verifying {}...", batch.id);
    let mut persist_state = |current_state: &MigrationState| context.persist_state(current_state);
    let mut check_shutdown = || context.check_shutdown();
    if let Err(err) = verify_batch_with_state_updates(
        batch,
        &context.config.source,
        &context.config.dest,
        state,
        &mut persist_state,
        &mut check_shutdown,
        &|batch_id| {
            CaravanError::StateCorrupt(format!(
                "batch {} disappeared from state during verification",
                batch_id
            ))
        },
    ) {
        if let CaravanError::VerificationFailed(failure) = &err {
            print_verification_failed(failure);
            crate::source_completion::remove_batch_completed(
                &context.config.source,
                &state.mode,
                batch,
            )
            .map_err(|remove_err| {
                CaravanError::StateCorrupt(format!(
                    "verification failed for {} and completed-file ledger cleanup failed: {}",
                    batch.id, remove_err
                ))
            })?;
        }
        return Err(err);
    }

    print_resume_verification_passed();
    Ok(())
}

fn copy_batch_for_resume(
    batch: &Batch,
    context: &ResumeContext<'_>,
    state: &mut MigrationState,
) -> Result<(), CaravanError> {
    print_resume_processing_batch_banner(batch);
    ensure_destination_capacity(&context.config.dest, batch.total_bytes)?;

    let conflict_report = crate::conflict::detect_batch_conflicts(batch, &context.config.dest)?;
    if conflict_report.has_conflicts {
        match effective_conflict_policy(context) {
            ConflictPolicy::SkipBatch => {
                mark_batch_failed_for_conflicts(batch, state, context, &conflict_report, 0)?;
                return Err(CaravanError::PolicyBlocked(format!(
                    "batch {} requires operator review before continuing: naming conflicts={} size_mismatches={}",
                    batch.id,
                    conflict_report.total_conflicts,
                    conflict_report.size_mismatches.len()
                )));
            }
            ConflictPolicy::SkipFile => {
                let copy_subset =
                    non_conflicting_subset_batch(batch, &context.config.dest, &conflict_report);
                if copy_subset.files.is_empty() {
                    mark_batch_failed_for_conflicts(batch, state, context, &conflict_report, 0)?;
                    return Err(CaravanError::PolicyBlocked(format!(
                        "batch {} requires operator review before continuing: naming conflicts={} size_mismatches={}",
                        batch.id,
                        conflict_report.total_conflicts,
                        conflict_report.size_mismatches.len()
                    )));
                }

                let mut persist_state =
                    |current_state: &MigrationState| context.persist_state(current_state);
                let mut check_shutdown = || context.check_shutdown();
                copy_batch_with_state_updates(
                    &copy_subset,
                    state,
                    CopyBatchOp {
                        source_root: &context.config.source,
                        dest_root: &context.config.dest,
                        copy_backend: &context.copy_backend,
                        reset_verification_passed: false,
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
                return Err(CaravanError::PolicyBlocked(format!(
                    "batch {} requires operator review before continuing: naming conflicts={} size_mismatches={}",
                    batch.id,
                    conflict_report.total_conflicts,
                    conflict_report.size_mismatches.len()
                )));
            }
        }
    }

    let mut persist_state = |current_state: &MigrationState| context.persist_state(current_state);
    let mut check_shutdown = || context.check_shutdown();
    copy_batch_with_state_updates(
        batch,
        state,
        CopyBatchOp {
            source_root: &context.config.source,
            dest_root: &context.config.dest,
            copy_backend: &context.copy_backend,
            reset_verification_passed: false,
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
    crate::source_completion::mark_batch_completed(&context.config.source, &state.mode, batch)?;

    Ok(())
}

pub(super) fn execute_resume_step(
    step: resume_ops::ResumeStepPlan,
    batch: &Batch,
    context: &ResumeContext<'_>,
    state: &mut MigrationState,
) -> Result<(), CaravanError> {
    match step {
        resume_ops::ResumeStepPlan::BatchFullyCompleted
        | resume_ops::ResumeStepPlan::PostDeleteSnapshot => {
            print_resume_skip_already_completed(&batch.id);
            Ok(())
        }
        resume_ops::ResumeStepPlan::ConflictOperatorReview { reason } => {
            Err(CaravanError::PolicyBlocked(format!(
                "batch {} requires operator review before continuing: {}",
                batch.id, reason
            )))
        }
        resume_ops::ResumeStepPlan::BlockedFailedVerification => {
            Err(CaravanError::PolicyBlocked(format!(
                "batch {} failed verification and requires operator review before continuing",
                batch.id
            )))
        }
        resume_ops::ResumeStepPlan::DeleteSource
        | resume_ops::ResumeStepPlan::PendingDeleteApproval => {
            print_resume_continue_to_deletion(&batch.id);
            Ok(())
        }
        resume_ops::ResumeStepPlan::VerifyBatch => verify_batch_for_resume(batch, context, state),
        resume_ops::ResumeStepPlan::CopyBatch => {
            copy_batch_for_resume(batch, context, state)?;
            verify_batch_for_resume(batch, context, state)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConflictPolicy, CopyStrategy, Mode, TransferConfig};
    use crate::models::file_entry::FileEntry;
    use crate::signal::ShutdownFlag;
    use crate::transfer::LocalFsCopyBackend;
    use std::path::PathBuf;

    fn sample_config(source: &std::path::Path, dest: &std::path::Path) -> TransferConfig {
        TransferConfig {
            mode: Mode::Staging,
            source: source.to_path_buf(),
            dest: dest.to_path_buf(),
            batch_size_bytes: 1024,
            max_files: None,
            snapshot_every: None,
            snapshot_dir: None,
            interactive: false,
            log_level: "info".to_string(),
            skip_conflicts: false,
            conflict_policy: ConflictPolicy::SkipBatch,
            recover_failed: false,
            allow_unsafe_filesystems: false,
            copy_strategy: CopyStrategy::Auto,
        }
    }

    fn sample_batch() -> Batch {
        Batch {
            id: "batch-000001".to_string(),
            files: vec![FileEntry {
                relative_path: PathBuf::from("a.txt"),
                size_bytes: 10,
                modified_time: None,
            }],
            total_bytes: 10,
            file_count: 1,
        }
    }

    fn sample_context<'a>(
        config: &'a TransferConfig,
        tmp: &tempfile::TempDir,
    ) -> ResumeContext<'a> {
        ResumeContext {
            config,
            state_path: tmp.path().join("state.json"),
            shutdown_flag: ShutdownFlag::new(),
            copy_backend: LocalFsCopyBackend::with_transfer_config(config),
            snapshot_backend: crate::snapshot::SystemSnapshotBackend,
        }
    }

    #[test]
    fn effective_conflict_policy_for_resume_recovery_forces_skip_file() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let source = tmp.path().join("source");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&source).expect("create source");
        std::fs::create_dir_all(&dest).expect("create dest");

        let mut config = sample_config(&source, &dest);
        config.recover_failed = true;
        let context = sample_context(&config, &tmp);

        assert_eq!(
            effective_conflict_policy(&context),
            ConflictPolicy::SkipFile
        );
    }

    #[test]
    fn non_conflicting_subset_batch_excludes_conflicting_paths() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let destination_root = tmp.path().join("dest");
        std::fs::create_dir_all(&destination_root).expect("create destination");

        let batch = Batch {
            id: "batch-000001".to_string(),
            files: vec![
                FileEntry {
                    relative_path: PathBuf::from("a.txt"),
                    size_bytes: 10,
                    modified_time: None,
                },
                FileEntry {
                    relative_path: PathBuf::from("b.txt"),
                    size_bytes: 20,
                    modified_time: None,
                },
            ],
            total_bytes: 30,
            file_count: 2,
        };
        let report = crate::conflict::ConflictReport {
            existing_files: vec![destination_root.join("b.txt")],
            size_mismatches: Vec::new(),
            total_conflicts: 1,
            has_conflicts: true,
            scanned_parent_directories: 1,
        };

        let subset = non_conflicting_subset_batch(&batch, &destination_root, &report);
        assert_eq!(subset.file_count, 1);
        assert_eq!(subset.total_bytes, 10);
        assert_eq!(subset.files[0].relative_path, PathBuf::from("a.txt"));
    }

    #[test]
    fn execute_resume_step_returns_policy_blocked_for_conflict_review() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let source = tmp.path().join("source");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&source).expect("create source");
        std::fs::create_dir_all(&dest).expect("create dest");
        let config = sample_config(&source, &dest);
        let context = sample_context(&config, &tmp);
        let batch = sample_batch();
        let mut state = MigrationState::new("staging", "/src", "/dst");

        let err = execute_resume_step(
            resume_ops::ResumeStepPlan::ConflictOperatorReview {
                reason: "conflict".to_string(),
            },
            &batch,
            &context,
            &mut state,
        )
        .expect_err("conflict step should block");

        assert!(err.to_string().contains("requires operator review"));
    }
}
