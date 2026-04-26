# Phase 1 Coverage Report (2026-04-26)

## Scope
- Analyze current automated test health.
- Generate baseline coverage artifacts for the project.
- Identify highest-risk uncovered areas before mutation testing.

## Environment and Tooling
- Rust: `rustc 1.96.0-nightly (c75612477 2026-04-07)`
- Cargo: `cargo 1.96.0-nightly (a357df4c2 2026-04-03)`
- Coverage instrumentation: `-Cinstrument-coverage`
- LLVM tools: rustup toolchain-matched `llvm-profdata` / `llvm-cov` from nightly sysroot

## Commands Executed
- Baseline reliability run:
  - `cargo test --all-targets --all-features -q`
- Test inventory count:
  - `cargo test --all-targets --all-features -- --list | rg -c ': test'`
- Coverage run:
  - `CARGO_TARGET_DIR=target/coverage-build RUSTFLAGS='-Cinstrument-coverage' LLVM_PROFILE_FILE='caravan-prof-%p-%m.profraw' cargo test --all-targets --all-features -q`
- Coverage artifacts generation:
  - `llvm-profdata merge ... -> target/coverage-artifacts/coverage.profdata`
  - `llvm-cov report ... -> target/coverage-artifacts/coverage-summary.txt`
  - `llvm-cov export --format=lcov ... -> target/coverage-artifacts/lcov.info`
  - `llvm-cov show --format=html ... -> target/coverage-artifacts/html/`

## Baseline Test Health
- Result: pass
- Count: **329 tests**
- Measured runtime (full suite): **21.140s**
- Flaky behavior observed in this run: none

## Coverage Results
- Overall line coverage (includes `src/` + `tests/`): **79.07%**
- `src/` only line coverage: **47.31%** (`2272/4802` lines)

### Critical Path Snapshot (`src/`)
- `src/transfer.rs`: **69.47%**
- `src/resume.rs`: **90.45%**
- `src/state_store.rs`: **63.46%**
- `src/capacity/mod.rs`: **84.75%**
- `src/capacity/linux.rs`: **61.29%**
- `src/preflight/mod.rs`: **75.34%**
- `src/preflight/platform_linux.rs`: **0.00%**
- `src/verify.rs`: **87.23%**

### Notable Low Coverage (`src/`, ascending)
- `src/preflight/platform_linux.rs`: **0.00%**
- `src/prompt.rs`: **35.43%**
- `src/plan.rs`: **51.53%**
- `src/atomic_write.rs`: **56.32%**
- `src/cli.rs`: **57.14%**
- `src/snapshot.rs`: **59.71%**
- `src/capacity/linux.rs`: **61.29%**
- `src/state_store.rs`: **63.46%**

### Entire Uncovered Cluster (0% in `src/`)
- `src/cli/commands/resume/batch_flow.rs`
- `src/cli/commands/resume/config.rs`
- `src/cli/commands/resume/context.rs`
- `src/cli/commands/resume/mod.rs`
- `src/cli/commands/resume/step_handlers.rs`
- `src/cli/commands/shared/app_context.rs`
- `src/cli/commands/shared/batch_ops.rs`
- `src/cli/commands/shared/capacity_guard.rs`
- `src/cli/commands/shared/deletion.rs`
- `src/cli/commands/shared/operator_review.rs`
- `src/cli/commands/shared/output.rs`
- `src/cli/commands/status.rs`
- `src/cli/commands/transfer/batch_handlers.rs`
- `src/cli/commands/transfer/context.rs`
- `src/cli/commands/transfer/copy_phase.rs`
- `src/cli/commands/transfer/delete_phase.rs`
- `src/cli/commands/transfer/mod.rs`
- `src/cli/commands/transfer/setup.rs`
- `src/cli/commands/transfer/verify_phase.rs`
- `src/logging.rs`
- `src/preflight/platform_linux.rs`
- `src/scan/linux.rs`

## Interpretation
- The suite is broad and stable, but coverage is heavily concentrated in integration tests and core helpers.
- `src/` coverage being 47.31% indicates large orchestration paths are unexecuted by tests.
- Main risk area before mutation testing: CLI orchestration and Linux-specific modules that currently show 0% coverage.

## Artifacts
- Summary table: `target/coverage-artifacts/coverage-summary.txt`
- LCOV: `target/coverage-artifacts/lcov.info`
- HTML coverage: `target/coverage-artifacts/html/index.html`
- Raw profile list: `target/coverage-artifacts/profraw-files.txt`
- Object list used: `target/coverage-artifacts/object-files.txt`

## Recommended Gate Before Phase 2
1. Add targeted tests for CLI command orchestration modules (`src/cli/commands/**`).
2. Add Linux-path tests for `preflight/platform_linux.rs` and `scan/linux.rs`.
3. Raise `src/` line coverage from **47.31%** to at least **65%** before enabling mutation scoring gates.
