# caravan Implementation Plan

## Purpose

Build `caravan`, a Rust command-line tool for a two-phase migration on a dual-boot Windows + CachyOS machine:

1. **Windows staging phase** — run `caravan` in staging mode while booted into Windows to copy data out of the Windows Storage Spaces volume into one or more Linux-readable intermediate volumes.
2. **Linux final migration phase** — reboot into CachyOS and run `caravan` in migration mode to move the staged data into a Btrfs destination using verified batches, explicit human approval gates, resumable checkpoints, and optional snapshots.

The tool is intentionally conservative. Its primary goal is data safety and recoverability, not maximum throughput.

---

## Corrected end-to-end migration architecture

### Realistic data flow for this machine

```text
Windows Storage Spaces source
        ↓
caravan staging mode on Windows
        ↓
Linux-readable intermediate volume(s)
        ↓
Reboot into CachyOS
        ↓
caravan migration mode on Linux
        ↓
Btrfs destination
```

### Important implications

- `caravan` must support **two execution contexts**:
  - **staging mode** on Windows
  - **migration mode** on Linux
- The same core planning, verification, approval, and checkpoint logic should be reusable across both modes.
- Btrfs-specific features such as snapshots are **Linux-only**.
- The code should remain mostly path-based and filesystem-agnostic at the batch orchestration level, while using platform-specific backends for copy, filesystem statistics, and snapshot commands.
- The tool should not depend on SMB for this workflow.

---

## Scope and core requirements

### Primary workflow in both modes

1. Read files from a source path.
2. Group files into configurable batches by total size and, optionally, by file count limits.
3. Check whether the destination has enough free space for the next batch.
4. Copy a batch to the destination.
5. Verify the copied batch.
6. If verification finds any anomaly, stop and require human review before any destructive action.
7. If verification passes, request human approval before deleting source files.
8. If approved, delete only the files in that batch from the source.
9. Optionally create a snapshot after a configurable milestone, but only in Linux migration mode.
10. Persist state after each meaningful step so the tool can resume safely.

### Mode-specific operational assumptions

#### Windows staging mode

- Source: Windows Storage Spaces volume exposed as a normal Windows drive or path.
- Destination: Linux-readable intermediate volume such as NTFS or exFAT.
- Copy backend: Windows-compatible file copy backend.
- Snapshotting: disabled.
- Capacity guard: checks free space on the intermediate destination before each batch.
- Deletion: removes files from the Storage Spaces source only after verification and explicit approval.

#### Linux migration mode

- Source: mounted staged data from the intermediate volume.
- Destination: mounted Btrfs filesystem.
- Copy backend: Linux-compatible copy backend, preferably `rsync` or an equivalent robust filesystem-aware transfer path.
- Snapshotting: enabled and configurable.
- Capacity guard: checks free space on the Btrfs destination before each batch.
- Deletion: removes files from the staged source only after verification and explicit approval.

### Safety invariants

- Never delete source data before a successful copy and verification step.
- Never delete source data after verification errors unless an explicitly designed override exists and is intentionally enabled.
- Never assume batch state from memory only; persist checkpoints to disk.
- Treat partial copies, I/O failures, and verification mismatches as normal failure modes.
- Make every destructive action require an explicit human decision in interactive mode.
- Abort a batch before copying if destination free space is less than or equal to the planned batch size.
- No agent, process, or automation may move, delete, or modify real filesystem data without explicit operator approval.

### Operator guardrail: real filesystem protection

- By default, all filesystem-mutating operations are prohibited until the operator explicitly approves execution.
- Planning, dry runs, and design validation must be read-only unless approval is granted for a controlled execution step.
- Any test execution for this project must run in a controlled environment and use mocked/synthetic data only.
- Tests must never target real migration sources or real destination datasets.
- If approval is absent, the safe default behavior is to stop and report that execution is blocked by policy.

### Destination routing determinism

- If multiple destination roots are supported, destination selection order must be deterministic and configurable.
- The selected destination root for each batch must be persisted in state/manifest before transfer starts.
- If free space changes between planning and transfer, capacity checks at execution time are authoritative.
- If a planned destination becomes unsafe, the planner may only re-route according to deterministic configured order and must persist the re-route decision before copy.
- If no destination can safely fit the batch, execution pauses and requires explicit operator intervention.

### Non-goals for v1

- Native Storage Spaces integration beyond ordinary Windows filesystem access.
- SMB-based same-machine transfer.
- Parallel transfer across multiple batches.
- Graphical user interface.
- Writing a custom copy engine from scratch.
- Full Btrfs management beyond snapshot creation and related helper calls.

---

## Implementation principles

- Use small, testable modules.
- Write tests before implementation for each phase.
- Keep external system interactions behind narrow interfaces so they can be mocked.
- Prefer deterministic behavior and explicit state transitions.
- Use existing tools where appropriate rather than reimplementing them.
- Fail closed: when uncertain, stop and require user attention.
- Prefer interactive confirmation for destructive actions, with scriptable execution available only when still gated by explicit approval artifacts or flags.

---

## Batching and locality strategy

The dataset is mostly large files, but there is a meaningful amount of small-file content under directories such as `Documents`.

To avoid pathological batches:

- cap batches by **total bytes** and **file count**
- prefer keeping files from the same directory tree together when possible
- allow directory-aware planning so many small files do not dominate a single batch indefinitely
- let the planner split very large folders into multiple batches while preserving local ordering where practical

Recommended initial defaults:

- batch size around `100GiB` for large-file areas
- smaller batches, such as `10–25GiB`, for heavily nested small-file areas if the planner supports directory-aware grouping
- a conservative file-count ceiling per batch to avoid huge metadata-only batches

---

## Verification strategy

The verification model should be tiered rather than binary.

### Tier 1: structural verification

Always perform this.

- file count
- byte count
- expected relative paths
- presence/absence checks
- source/destination stat comparisons

### Tier 2: digest verification

Default conservative choice.

- BLAKE3 for fast, strong file integrity checks
- full-file digest for smaller or critical files
- configurable sampling or partial hashing only if explicitly enabled and documented

### Tier 3: strict verification

For the most sensitive batches.

- full digest of every file in the batch
- repeatable verification before deletion

### Suggested policy

- small files: full digest by default
- medium files: full digest or strong sample-based policy, depending on operator configuration
- large files: full digest when safety matters most; sampling only if the operator accepts the tradeoff

A verification failure must pause execution and require human review before any deletion.

---

## Cross-platform abstraction model

The core engine should avoid platform-specific assumptions.

### Platform abstraction boundaries

- `copy_backend` — Windows copy vs Linux copy implementation
- `fs_probe` — free-space and filesystem-statistics provider
- `snapshot_backend` — Btrfs snapshot support on Linux only
- `prompt_backend` — interactive confirmation flow
- `journal_store` — state persistence
- `path_policy` — filename/path validation and compatibility checks

### Path policy and sanitization

The tool should validate for risky path patterns and report them clearly:

- reserved device names on Windows
- path length pressure
- trailing dots/spaces on Windows-facing paths
- case-collision risks when moving across filesystems with different semantics
- unsupported special files if encountered

For this migration, the sanitization layer should primarily **detect and report**, not silently rename. Automatic renaming should be an explicit, separately reviewed feature if it is ever added.

---

## Suggested repository layout

```text
caravan/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── cli.rs
│   ├── config.rs
│   ├── error.rs
│   ├── models/
│   │   ├── mod.rs
│   │   ├── file_entry.rs
│   │   ├── batch.rs
│   │   ├── state.rs
│   │   └── verification.rs
│   ├── scan.rs
│   ├── plan.rs
│   ├── capacity.rs
│   ├── transfer.rs
│   ├── verify.rs
│   ├── prompt.rs
│   ├── snapshot.rs
│   ├── cleanup.rs
│   ├── state_store.rs
│   ├── logging.rs
│   ├── resume.rs
│   └── platform/
│       ├── mod.rs
│       ├── windows.rs
│       └── linux.rs
├── tests/
│   ├── cli_tests.rs
│   ├── planning_tests.rs
│   ├── capacity_tests.rs
│   ├── state_tests.rs
│   ├── verification_tests.rs
│   ├── prompt_tests.rs
│   └── integration_smoke.rs
├── readme.md
├── manual.md
└── plan.md
```

---

## Data model overview

### FileEntry

Represents one source file.

Fields to include:

- relative path from the source root
- byte size
- modified time
- file type flags if needed (regular file, symlink, directory, special file)
- digest fields if used by verification

### Batch

Represents one planned migration unit.

Fields to include:

- batch id
- source root
- destination root
- list of file entries
- aggregate size
- aggregate file count
- status
- creation timestamp
- verification summary
- approval decision
- deletion result
- snapshot result

### MigrationState

Represents the persisted checkpoint.

Fields to include:

- tool version
- mode (`staging` or `migrate`)
- source path
- destination path
- batch size setting
- max file count per batch, if configured
- snapshot frequency setting
- verification mode
- current phase
- batches already completed
- batches pending
- last successful snapshot name
- log file path or log metadata
- resume metadata

### VerificationReport

Represents the result of verifying a batch.

Fields to include:

- file count
- bytes compared
- missing files
- mismatched files
- unreadable files
- digest mode used
- process exit status or error category
- overall pass/fail status
- recommended action

### CapacityReport

Represents the destination capacity check.

Fields to include:

- total destination capacity
- available free space
- planned batch size
- reserve margin if configured
- decision (`proceed` / `abort`)
- reason string for aborts

---

# Phase 0: Operational Preconditions and Migration Model Lock

## Goal

Lock the real-world workflow before writing code.

## Deliverables

- A written runbook section describing the Windows staging copy and the Linux final migration.
- A clarified statement that `caravan` runs in both contexts but with different backend capabilities.
- A finalized command model with explicit staging and migration modes.
- A finalized source/destination topology for each mode.

## Required operational procedure

### Windows staging phase

1. Boot into Windows.
2. Run `caravan` in staging mode against the Windows Storage Spaces source.
3. Copy the selected batch to a Linux-readable intermediate volume.
4. Verify the copy.
5. Review any verification errors.
6. Approve deletion only if the batch is safe to delete from Storage Spaces.
7. Repeat until the Storage Spaces source has been reduced enough for the final layout.

### Linux final migration phase

1. Boot into CachyOS.
2. Mount the staged intermediate volume.
3. Run `caravan` in migration mode against the staged data.
4. Copy into the Btrfs destination.
5. Verify the copy.
6. Review any verification errors.
7. Approve deletion of the staged source batch only if safe.
8. Create snapshots according to the configured cadence.

## Acceptance checks for this phase

- The data can be accessed from Windows staging mode.
- The staged files can be seen from CachyOS after reboot.
- The chosen intermediate path is stable enough to be used as the source for Linux migration mode.
- The plan clearly distinguishes between staging mode and migration mode.

## Phase completion criteria

- The real migration path is unambiguous.
- No implementation detail assumes the source is directly accessible from Linux before staging.
- The rest of the plan is written against the corrected dual-mode model.

---

# Phase 1: Project Scaffolding and Build Foundations

## Goal

Create the Rust project structure, dependency set, formatting/linting baseline, and module skeletons.

## Deliverables

- `Cargo.toml` with chosen dependencies.
- `src/lib.rs` and `src/main.rs`.
- Module skeletons with stub functions.
- Formatting and linting configuration.
- Basic error type.

## Recommended dependencies

- `clap` for CLI parsing.
- `serde` and `serde_json` for state persistence.
- `thiserror` for error definitions.
- `anyhow` only if needed for top-level ergonomics; prefer typed errors inside the library.
- `tracing` or `log` + `env_logger` for logging.
- `tempfile` for tests.
- `assert_cmd` and `predicates` for CLI tests.
- `mockall` or custom trait mocks for process execution abstraction.
- `blake3` for fast integrity verification.

## Unit tests to write before code

1. The crate compiles with an empty module graph.
2. Public error types can be constructed and matched.
3. The binary entrypoint exists and returns a controlled failure for missing arguments.
4. Module boundaries can be imported from the library crate.
5. Formatter/linter configuration does not break the test suite.

## Completion criteria

- `cargo test` passes with stub modules.
- The crate builds in debug mode.
- A minimal binary invocation yields expected CLI help or validation behavior.

---

# Phase 2: CLI Parsing and Configuration Model

## Goal

Define and validate all command-line options and map them into a typed configuration structure.

## Deliverables

- CLI subcommands or command structure.
- Configuration struct.
- Parsing for source, destination, batch size, file-count cap, snapshot cadence, interactive mode, verification mode, resume mode, and logging options.
- Validation rules for invalid combinations.

## Recommended CLI shape

Prefer explicit subcommands:

```text
caravan staging --source <PATH> --dest <PATH> --batch-size 100GiB --interactive
caravan migrate --source <PATH> --dest <PATH> --batch-size 100GiB --snapshot-every 2 --interactive
caravan status
caravan resume
```

If a convenience flag such as `--mode staging` is ever added, it should remain an alias rather than the primary shape.

## Tests to write

- missing required arguments are rejected
- invalid batch sizes are rejected
- invalid snapshot settings in staging mode are rejected
- interactive mode is enabled and disabled correctly
- state file path defaults are applied correctly
- source and destination ordering is validated by mode
- mutually exclusive flags are rejected

## Completion criteria

- CLI argument parsing is stable and deterministic.
- The parsed configuration can be serialized into a canonical internal config.
- Mode-specific invalid combinations fail early.

---

# Phase 3: File Scanning and Batch Planning

## Goal

Scan the source tree and create batches that respect size, file-count, and locality constraints.

## Deliverables

- Recursive scanner.
- File metadata capture.
- Batch planner.
- Deterministic batch ordering.
- Support for resumable planning snapshots.

## Planning behavior

- Group files by directory locality when possible.
- Keep batches under the configured size threshold.
- Respect a file-count ceiling when many small files are present.
- Preserve deterministic ordering so state can be resumed predictably.
- Prefer batches that are easy for an operator to reason about.

## Tests to write

- empty source tree produces zero batches
- single large file forms one batch
- many small files are split into multiple batches by file count and size
- directory-local files stay together when possible
- planning is deterministic across runs
- source mutations between scan and execution are detected later by verification or source-stability checks

## Completion criteria

- Planned batches are reproducible.
- Batches respect size and file-count limits.
- The planner emits enough metadata for later verification and resumption.

---

# Phase 4: Capacity Checks and Guardrails

## Goal

Prevent unsafe copies by checking destination capacity before each batch.

## Deliverables

- Destination free-space probe.
- Batch-vs-space decision logic.
- Reserve margin support if configured.
- Clear abort reasons.

## Guardrail rules

- Abort before copy if destination free space is less than or equal to the planned batch size.
- Optional reserve margin can make this stricter.
- In Linux migration mode, account for any temporary working space required by the copy backend.

## Tests to write

- free space greater than batch size allows copy
- free space equal to batch size aborts
- free space less than batch size aborts
- reserve margin is applied correctly
- capacity failures are written to state and logs

## Completion criteria

- Unsafe batches are blocked before transfer begins.
- Capacity behavior is mode-aware and easy to audit.

---

# Phase 5: Transfer Backend and Verification

## Goal

Copy a batch safely and verify it before any deletion.

## Deliverables

- Copy backend abstraction.
- Windows copy implementation.
- Linux copy implementation.
- Verification engine.
- Verification report serialization.

## Verification approach

- Always verify structure and counts.
- Verify digests using BLAKE3 by default.
- Allow stricter verification for sensitive batches.
- Pause and require human review on any mismatch.

## Tests to write

- copied file contents match the source
- missing files fail verification
- size mismatch fails verification
- digest mismatch fails verification
- unreadable file fails verification
- copy interruption leaves resumable state behind

## Completion criteria

- Copy and verification are decoupled.
- Verification failures never fall through into deletion.
- Verification reports are understandable and persistent.

---

# Phase 6: Approval Flow, Deletion, and Journaling

## Goal

Require explicit human approval before any destructive action and record every step.

## Deliverables

- interactive prompt flow
- approval token or approval state representation
- deletion executor
- journal/state store

## Approval model

- Verification pass does not imply deletion.
- The operator must approve each batch or each batch group explicitly.
- Any verification error stops the run and requires human attention.
- The state store must remember the batch state across power loss and process restarts.
- In non-interactive or scripted mode, destructive operations remain disabled unless an explicit approval artifact or approval flag is provided.
- If interactive confirmation is unavailable and no explicit approval mechanism is present, fail closed and stop before deletion.
- Approval decisions must be journaled with timestamp, batch id, and execution context.

## Tests to write

- approval is required before deletion
- deletion is blocked when verification fails
- approval state is persisted
- resumed runs do not repeat already-completed destructive steps
- interrupted deletions are recoverable or reported as incomplete

## Completion criteria

- Destructive steps are never implicit.
- Batch state is recoverable from disk.
- The tool can be safely resumed after interruption.

---

# Phase 7: Linux Snapshots and Final Migration Behavior

## Goal

Add Btrfs snapshot support in Linux migration mode only.

## Deliverables

- snapshot backend abstraction
- Btrfs snapshot helper
- snapshot cadence configuration
- snapshot metadata persisted in state

## Snapshot rules

- Disabled in staging mode.
- Enabled only in migration mode.
- Snapshot creation should occur after approved batch completion or at a configured cadence.
- Snapshot failures should be reported clearly and should not cause silent deletion.

## Tests to write

- snapshots are rejected in staging mode
- snapshot creation is invoked only in migration mode
- snapshot failures are recorded
- snapshot metadata is persisted

## Completion criteria

- Snapshot behavior is platform-appropriate.
- Snapshot actions do not weaken the deletion gate.

---

# Phase 8: Resume Model and Failure Handling

## Goal

Make every important step recoverable.

## Deliverables

- resume logic
- state reconciliation
- failure classification
- operator-facing recovery messages

## Failure classes to handle

- I/O errors
- verification mismatches
- copy backend failures
- destination capacity exhaustion
- corrupted or unreadable state file
- source tree changes between planning and execution

## Resume boundaries and idempotency rules

- Persist state before and after each critical boundary: `planned`, `copy_started`, `copy_completed`, `verify_completed`, `approved_for_delete`, `delete_completed`, and `snapshot_completed`.
- Re-running from the same checkpoint must not duplicate destructive operations.
- Partial-copy detection must reconcile destination state before retrying a batch.
- Deletion must only run when both verification pass and explicit approval are present in persisted state.
- If state and filesystem observations conflict, stop and require operator review instead of guessing.

## Tests to write

- resume after copy but before verify
- resume after verify but before delete
- resume after delete but before snapshot
- resume with missing state file fails cleanly
- resume after a partial batch does not corrupt state
- non-interactive resume without approval artifacts fails closed before deletion

## Completion criteria

- The tool can restart from the last safe checkpoint.
- Failure messages are actionable rather than vague.

## Failure and recovery matrix

- `copy_backend_failure`: mark batch as failed, keep source intact, require operator retry or skip decision.
- `verification_mismatch`: block deletion, emit report, require operator review before any further destructive step.
- `capacity_drop_before_copy`: abort batch before transfer, persist reason, allow reroute or later retry.
- `state_file_unreadable`: stop startup, offer recovery path from backup/checkpoint export.
- `power_loss_mid_batch`: resume from last checkpoint, reconcile partial destination artifacts, and avoid duplicate deletes.

---

# Phase 9: Docs, Examples, and Operator Polish

## Goal

Make the workflow easy to follow and hard to misuse.

## Deliverables

- `readme.md`
- `manual.md`
- usage examples
- safe defaults documentation
- a clear explanation of interactive vs scriptable operation

## Completion criteria

- The documentation matches the implemented behavior.
- The operator can understand the full workflow without reading the code.
- The safety gates are explicit in the docs.

