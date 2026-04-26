# Unit Coverage Update (2026-04-26)

## Cleanup Completed
- Removed root-scattered coverage artifacts before implementing tests.
- Root `.profraw` files now: `0`.

## What Was Added
Unit tests were added directly in `src/` modules (not integration tests) with focus on low-coverage command/runtime paths:

- `src/logging.rs`
- `src/preflight/platform_linux.rs`
- `src/scan/linux.rs`
- `src/cli/commands/resume/config.rs`
- `src/cli/commands/resume/context.rs`
- `src/cli/commands/resume/batch_flow.rs`
- `src/cli/commands/resume/step_handlers.rs`
- `src/cli/commands/resume/mod.rs`
- `src/cli/commands/shared/operator_review.rs`
- `src/cli/commands/shared/app_context.rs`
- `src/cli/commands/shared/capacity_guard.rs`
- `src/cli/commands/shared/deletion.rs`
- `src/cli/commands/transfer/mod.rs`
- `src/cli/commands/transfer/setup.rs`
- `src/cli/commands/transfer/context.rs`
- `src/cli/commands/transfer/copy_phase.rs`
- `src/cli/commands/transfer/verify_phase.rs`
- `src/cli/commands/transfer/delete_phase.rs`
- `src/cli/commands/transfer/batch_handlers.rs`

## Validation
- `cargo fmt` passed.
- `cargo test -q` passed.
- `scripts/check_cfg_policy.sh` passed.

## Coverage Snapshot After Unit-Test Pass
Using the same instrumentation approach as Phase 1:

- Overall line coverage (all files): **92.39%**
- `src/` line coverage: **83.88%** (`4798/5720`)

Notable previously low/zero modules now covered:
- `src/cli/commands/resume/config.rs`: **98.53%**
- `src/cli/commands/resume/context.rs`: **100.00%**
- `src/cli/commands/resume/batch_flow.rs`: **90.82%**
- `src/cli/commands/resume/step_handlers.rs`: **83.17%**
- `src/cli/commands/transfer/mod.rs`: **92.00%**
- `src/cli/commands/transfer/setup.rs`: **98.49%**
- `src/cli/commands/transfer/context.rs`: **100.00%**
- `src/cli/commands/transfer/copy_phase.rs`: **95.70%**
- `src/cli/commands/transfer/verify_phase.rs`: **97.41%**
- `src/cli/commands/transfer/delete_phase.rs`: **100.00%**
- `src/cli/commands/shared/operator_review.rs`: **100.00%**
- `src/scan/linux.rs`: **100.00%**
- `src/logging.rs`: **93.18%**

Still comparatively lower and worth another unit-test pass:
- `src/preflight/platform_linux.rs`: **63.24%**

## Second Unit-Focused Pass (platform_linux.rs)
- Added targeted unit tests for mountinfo parsing branches (longest mount match, malformed lines, root fallback, octal edge cases).
- Updated coverage snapshot:
  - Overall line coverage (all files): **92.57%**
  - `src/` line coverage: **84.36%** (`4861/5762`)
  - `src/preflight/platform_linux.rs`: **83.71%** line coverage

## Third Unit-Focused Pass (Core Runtime Helpers)
Added in-file unit tests for branch-heavy runtime helpers:

- `src/atomic_write.rs`
- `src/snapshot.rs`
- `src/progress.rs`
- `src/capacity/linux.rs`
- `src/cleanup.rs`
- `src/size.rs`
- `src/status.rs`
- `src/state_discovery.rs`
- `src/prompt.rs`

Validation:
- `cargo fmt` passed.
- `cargo test -q` passed.
- `scripts/check_cfg_policy.sh` passed.

### Unit-Only Coverage Snapshot (`cargo test --lib`)
Because subprocesses spawned by integration tests can lose `.profraw` writes when running from temporary working directories, this snapshot is generated from the instrumented **lib test harness** (`cargo test --lib`) to measure unit-test impact directly.

- Unit-only overall line coverage: **52.09%** (`3339/6410`)
  - LLVM summary total lines: `6410`
  - LLVM summary missed lines: `3071`

Key module results after this pass:
- `atomic_write.rs`: **78.99%**
- `capacity/linux.rs`: **86.96%**
- `snapshot.rs`: **77.76%**
- `progress.rs`: **70.66%**
- `prompt.rs`: **69.33%**
- `cleanup.rs`: **94.37%**
- `size.rs`: **100.00%**
- `status.rs`: **100.00%**
- `state_discovery.rs`: **96.59%**
