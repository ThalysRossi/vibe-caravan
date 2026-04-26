use crate::error::CaravanError;
use crate::models::state::MigrationState;
use crate::signal::check_shutdown;

use super::super::shared::{approve_and_delete_verified_batches, ensure_no_operator_review_blocks};
use super::context::TransferContext;

pub(super) fn run_delete_phase(
    context: &TransferContext<'_>,
    state: &mut MigrationState,
) -> Result<(), CaravanError> {
    check_shutdown(&context.shutdown_flag)?;
    ensure_no_operator_review_blocks(state)?;

    let mut persist_state = |current_state: &MigrationState| context.persist_state(current_state);
    approve_and_delete_verified_batches(
        state,
        &context.config.source,
        context.config.interactive,
        &context.shutdown_flag,
        &mut persist_state,
        "execute_transfer",
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConflictPolicy, CopyStrategy, Mode, TransferConfig};
    use crate::signal::ShutdownFlag;
    use crate::transfer::LocalFsCopyBackend;

    #[test]
    fn run_delete_phase_is_noop_when_no_batches_need_deletion() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let source = tmp.path().join("source");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&source).expect("create source");
        std::fs::create_dir_all(&dest).expect("create dest");

        let config = TransferConfig {
            mode: Mode::Staging,
            source: source.clone(),
            dest: dest.clone(),
            batch_size_bytes: 1024,
            max_files: None,
            snapshot_every: None,
            snapshot_dir: None,
            interactive: false,
            log_level: "info".to_string(),
            skip_conflicts: false,
            conflict_policy: ConflictPolicy::SkipFile,
            recover_failed: false,
            allow_unsafe_filesystems: false,
            copy_strategy: CopyStrategy::Auto,
        };
        let context = TransferContext {
            config: &config,
            state_path: tmp.path().join("state.json"),
            secondary_state_path: tmp.path().join("state_compat.json"),
            shutdown_flag: ShutdownFlag::new(),
            copy_backend: LocalFsCopyBackend::with_transfer_config(&config),
            snapshot_backend: crate::snapshot::SystemSnapshotBackend,
        };
        let mut state = MigrationState::new("staging", "/src", "/dst");

        run_delete_phase(&context, &mut state).expect("delete phase should succeed");
    }
}
