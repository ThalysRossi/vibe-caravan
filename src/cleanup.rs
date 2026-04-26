use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::state::{BatchPhase, JournalEntry, MigrationState};

pub trait FileRemover {
    fn remove_file(&self, path: &Path) -> std::io::Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FsFileRemover;

impl FileRemover for FsFileRemover {
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_file(path)
    }
}

pub fn cleanup_batch(
    batch: &Batch,
    source_root: &Path,
    state: &mut MigrationState,
    execution_context: &str,
) -> Result<(), CaravanError> {
    cleanup_batch_with_remover(batch, source_root, state, execution_context, &FsFileRemover)
}

pub fn cleanup_batch_with_remover(
    batch: &Batch,
    source_root: &Path,
    state: &mut MigrationState,
    execution_context: &str,
    remover: &dyn FileRemover,
) -> Result<(), CaravanError> {
    let batch_state = state.batch(&batch.id).cloned().ok_or_else(|| {
        CaravanError::StateCorrupt(format!(
            "missing batch state for {} before cleanup",
            batch.id
        ))
    })?;

    if !batch_state.verification_passed {
        return Err(CaravanError::PolicyBlocked(
            "deletion blocked because verification did not pass".to_string(),
        ));
    }
    if !batch_state.approved_for_delete {
        return Err(CaravanError::PolicyBlocked(
            "deletion blocked because batch is not approved".to_string(),
        ));
    }
    if batch_state.deleted {
        return Ok(());
    }

    state.journal.push(JournalEntry {
        event: "delete_started".to_string(),
        batch_id: batch.id.clone(),
        timestamp_unix_secs: now_unix_secs(),
        context: execution_context.to_string(),
    });

    for file in &batch.files {
        let path = source_root.join(&file.relative_path);
        if path.exists() {
            if let Err(err) = remover.remove_file(&path) {
                let mut failed = batch_state.clone();
                failed.phase = BatchPhase::Failed;
                failed.deleted = false;
                state.upsert_batch(failed);
                state.journal.push(JournalEntry {
                    event: "delete_failed".to_string(),
                    batch_id: batch.id.clone(),
                    timestamp_unix_secs: now_unix_secs(),
                    context: format!(
                        "{} | file={} | error={}",
                        execution_context,
                        path.display(),
                        err
                    ),
                });
                return Err(CaravanError::IoContext {
                    context: format!("failed to delete source file {}", path.display()),
                    source: err,
                });
            }
        }
    }

    let mut updated = batch_state.clone();
    updated.deleted = true;
    updated.phase = BatchPhase::DeleteCompleted;
    state.upsert_batch(updated);
    state.journal.push(JournalEntry {
        event: "delete_completed".to_string(),
        batch_id: batch.id.clone(),
        timestamp_unix_secs: now_unix_secs(),
        context: execution_context.to_string(),
    });

    Ok(())
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::RefCell;
    use std::path::PathBuf;

    use crate::models::file_entry::FileEntry;
    use tempfile::tempdir;

    struct RecordingRemover {
        fail_on: Option<PathBuf>,
        removed: RefCell<Vec<PathBuf>>,
    }

    impl RecordingRemover {
        fn new(fail_on: Option<PathBuf>) -> Self {
            Self {
                fail_on,
                removed: RefCell::new(Vec::new()),
            }
        }
    }

    impl FileRemover for RecordingRemover {
        fn remove_file(&self, path: &Path) -> std::io::Result<()> {
            if self.fail_on.as_ref() == Some(&path.to_path_buf()) {
                return Err(std::io::Error::other("simulated delete failure"));
            }
            self.removed.borrow_mut().push(path.to_path_buf());
            fs::remove_file(path)
        }
    }

    fn batch_with_files(id: &str, files: &[&str]) -> Batch {
        let file_entries: Vec<FileEntry> = files
            .iter()
            .map(|relative| FileEntry {
                relative_path: PathBuf::from(relative),
                size_bytes: 1,
                modified_time: None,
            })
            .collect();

        Batch {
            id: id.to_string(),
            file_count: file_entries.len(),
            total_bytes: file_entries.len() as u64,
            files: file_entries,
        }
    }

    fn state_with_batch(
        id: &str,
        verification_passed: bool,
        approved_for_delete: bool,
        deleted: bool,
    ) -> MigrationState {
        let mut state = MigrationState::new("migrate", "/src", "/dst");
        state.upsert_batch(crate::models::state::BatchState {
            batch_id: id.to_string(),
            phase: BatchPhase::VerifyCompleted,
            verification_passed,
            approved_for_delete,
            deleted,
        });
        state
    }

    #[test]
    fn cleanup_requires_existing_batch_state() {
        let temp = tempdir().expect("tempdir");
        let batch = batch_with_files("batch-1", &["a.txt"]);
        let mut state = MigrationState::new("migrate", "/src", "/dst");
        let remover = RecordingRemover::new(None);

        let err = cleanup_batch_with_remover(&batch, temp.path(), &mut state, "ctx", &remover)
            .expect_err("missing batch state must fail");
        assert!(err.to_string().contains("missing batch state"));
    }

    #[test]
    fn cleanup_blocks_when_verification_or_approval_are_missing() {
        let temp = tempdir().expect("tempdir");
        let batch = batch_with_files("batch-1", &["a.txt"]);
        let remover = RecordingRemover::new(None);

        let mut unverified = state_with_batch("batch-1", false, true, false);
        let err = cleanup_batch_with_remover(&batch, temp.path(), &mut unverified, "ctx", &remover)
            .expect_err("unverified batch must be blocked");
        assert!(err.to_string().contains("verification did not pass"));

        let mut unapproved = state_with_batch("batch-1", true, false, false);
        let err = cleanup_batch_with_remover(&batch, temp.path(), &mut unapproved, "ctx", &remover)
            .expect_err("unapproved batch must be blocked");
        assert!(err.to_string().contains("batch is not approved"));
    }

    #[test]
    fn cleanup_returns_ok_without_changes_when_batch_already_deleted() {
        let temp = tempdir().expect("tempdir");
        let batch = batch_with_files("batch-1", &["a.txt"]);
        let mut state = state_with_batch("batch-1", true, true, true);
        let remover = RecordingRemover::new(None);

        cleanup_batch_with_remover(&batch, temp.path(), &mut state, "ctx", &remover)
            .expect("already deleted should be a no-op");

        assert!(remover.removed.borrow().is_empty());
        assert!(state.journal.is_empty());
    }

    #[test]
    fn cleanup_deletes_existing_files_and_marks_batch_completed() {
        let temp = tempdir().expect("tempdir");
        let source_root = temp.path();
        let file_a = source_root.join("dir/a.txt");
        let file_b = source_root.join("dir/b.txt");
        fs::create_dir_all(file_a.parent().expect("parent")).expect("create parent");
        fs::write(&file_a, b"a").expect("seed a");
        fs::write(&file_b, b"b").expect("seed b");

        let batch = batch_with_files("batch-1", &["dir/a.txt", "dir/b.txt", "dir/missing.txt"]);
        let mut state = state_with_batch("batch-1", true, true, false);
        let remover = RecordingRemover::new(None);

        cleanup_batch_with_remover(&batch, source_root, &mut state, "unit-test", &remover)
            .expect("cleanup should succeed");

        assert!(!file_a.exists());
        assert!(!file_b.exists());
        assert_eq!(remover.removed.borrow().len(), 2);

        let batch_state = state.batch("batch-1").expect("batch state");
        assert!(batch_state.deleted);
        assert_eq!(batch_state.phase, BatchPhase::DeleteCompleted);
        assert_eq!(state.journal.len(), 2);
        assert_eq!(state.journal[0].event, "delete_started");
        assert_eq!(state.journal[1].event, "delete_completed");
    }

    #[test]
    fn cleanup_marks_batch_failed_and_records_journal_on_delete_error() {
        let temp = tempdir().expect("tempdir");
        let source_root = temp.path();
        let file = source_root.join("dir/a.txt");
        fs::create_dir_all(file.parent().expect("parent")).expect("create parent");
        fs::write(&file, b"a").expect("seed file");

        let batch = batch_with_files("batch-1", &["dir/a.txt"]);
        let mut state = state_with_batch("batch-1", true, true, false);
        let remover = RecordingRemover::new(Some(file.clone()));

        let err =
            cleanup_batch_with_remover(&batch, source_root, &mut state, "unit-test", &remover)
                .expect_err("delete failure should fail cleanup");
        assert!(err.to_string().contains("failed to delete source file"));

        let batch_state = state.batch("batch-1").expect("batch state");
        assert!(!batch_state.deleted);
        assert_eq!(batch_state.phase, BatchPhase::Failed);
        assert_eq!(state.journal.len(), 2);
        assert_eq!(state.journal[0].event, "delete_started");
        assert_eq!(state.journal[1].event, "delete_failed");
        assert!(
            state.journal[1]
                .context
                .contains("simulated delete failure"),
            "unexpected context: {}",
            state.journal[1].context
        );
    }
}
