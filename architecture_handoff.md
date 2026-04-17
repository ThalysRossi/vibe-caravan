# Caravan Architecture Handoff

This document consolidates the architecture analysis from the previous review messages:

1. Detailed architecture breakdown, rationale, and idiomatic Rust assessment.
2. Diagnosis of orphan/legacy files and why type/model placement is currently inconsistent.

---

## 1) Architecture Overview

Caravan is a **stateful, safety-first pipeline CLI**:

- Command layer orchestrates phases (`staging`, `migrate`, `resume`, `status`).
- Domain modules implement planning, scanning, copying, verification, cleanup, snapshots.
- Persistence and registry modules make runs resumable and crash-tolerant.
- Safety gates block destructive actions unless verification and approval policy pass.

### Layered Structure

#### A. CLI and Config Boundary

- CLI argument parsing and subcommand dispatch:
  - `src/cli.rs`
- Typed application config:
  - `src/config.rs`

This gives a clean boundary: parse external inputs once, then execute via typed internal config.

#### B. Orchestration Layer (Application Workflows)

- Transfer pipeline:
  - `src/cli/commands/transfer/mod.rs`
  - `copy_phase.rs`, `verify_phase.rs`, `delete_phase.rs`
- Resume workflow:
  - `src/cli/commands/resume/mod.rs`
  - `batch_flow.rs`, `step_handlers.rs`

This layer coordinates phase transitions, preflight checks, persistence hooks, and policy enforcement.

#### C. Domain State Machine and Contracts

- Core persisted state and phase model:
  - `src/models/state.rs`
- Resume decision engine:
  - `src/resume.rs`

Batch and migration phases are explicit and persisted, enabling deterministic crash recovery and safe resume.

#### D. Core Engines

- Planning and drift checks:
  - `src/plan.rs`
- Source traversal/scanning:
  - `src/scan.rs`
- Copy backend selection + atomic file finalize:
  - `src/transfer.rs`
- Verification:
  - `src/verify.rs`
- Snapshot cadence + execution:
  - `src/snapshot.rs`

#### E. Durability and Safety Infrastructure

- Atomic write discipline:
  - `src/atomic_write.rs`
- Versioned state envelope + checksum + dual-copy reconciliation:
  - `src/state_store.rs`
- Migration registry lifecycle:
  - `src/migration_registry.rs`
- Signal handling / graceful shutdown:
  - `src/signal.rs`
- Typed error taxonomy:
  - `src/error.rs`

---

## 2) Why It Is Structured This Way

The system optimizes for **data safety and resumability**, not just throughput:

1. **Recoverability**  
   Persisted phase transitions + resume planning from state.

2. **Fail-closed destructive behavior**  
   Delete only after verification + explicit policy/approval gates.

3. **Determinism under interruptions**  
   Immutable planned manifests are validated against current source snapshot before destructive steps.

4. **Crash consistency**  
   Atomic state writes, checksums, revisioned envelopes, and dual-copy reconciliation.

5. **Testability**  
   Trait seams for side effects (copy/snapshot/probe/prompt) isolate behavior from platform I/O.

For a migration tool handling real user data, this is the correct architectural priority.

---

## 3) Is This a Common Rust Pattern?

### What is idiomatic/common

- `clap` parsing + typed config conversion.
- `thiserror`-based error enum.
- Module-oriented orchestration over pure-ish domain functions.
- Trait-based abstractions around side effects for tests.
- `serde`-based persisted state with compatibility handling.

### What is less idiomatic at larger scale

1. Heavy free-function orchestration with many parameters instead of cohesive service/context objects.
2. Partial coupling between domain and presentation (`println!` in flow helpers).
3. Mixed placement criteria for types (some contracts in `models/`, others in feature modules without strict convention).

---

## 4) Architectural Improvement Direction (Future Session)

### A. Introduce an application context/service layer

Create an `AppContext` / `Services` struct that owns:

- `StateStore`
- `MigrationRegistry`
- `CopyBackend`
- `SnapshotBackend`
- `PromptBackend`
- Logging/clock abstractions

This removes closure/argument sprawl and clarifies ownership.

### B. Separate domain events from CLI rendering

- Domain returns structured events.
- CLI layer maps events to human/json outputs.

Benefit: clearer tests, cleaner command modules, easier future non-CLI integrations.

### C. Strengthen persisted type contracts

- Prefer typed enums/path wrappers in persisted structures where practical.
- Keep explicit compatibility adapters for old state versions.

### D. Improve batch state indexing

- Replace repeated vector lookups with indexed state structures (`HashMap` + ordered ids).

### E. Complete typed error flow

- Continue reducing stringly classification where still present.
- Keep stable operator-facing failure classes as a mapping layer over typed errors.

### F. Transactional consistency between state and registry

- Make registry status + state revision updates part of a single consistency protocol (or explicit recoverable intent model).

---

## 5) Orphan / Legacy File Analysis

### Key finding

Most top-level `src/*.rs` files are active domain modules and **not orphaned**.

The clearest legacy/orphan-like area is `src/detection.rs`:

- Runtime command flow does not currently use its main detection APIs directly.
- It is primarily exercised by tests.
- It overlaps responsibilities now owned elsewhere (notably size parsing behavior in CLI args).

Relevant files:

- Legacy-like module:
  - `src/detection.rs`
- Active parsing path:
  - `src/cli/args.rs`
- Prompt still coupled to detection enum:
  - `src/prompt.rs`

### Why this happened

This is typical of incremental refactors:

- Original behavior moved into command/config paths.
- Tests and a few type references kept old module alive.
- Module remained exported and compiled, but no longer sits on the main runtime path.

---

## 6) Why Some Types Live in `models/` and Others Don’t

Current pattern is mixed:

1. `models/` holds shared/persisted contracts (`Batch`, `FileEntry`, `MigrationState`, verification report types).
2. Feature modules define local operational types near logic (`FailureClass`, preflight warning structs, capacity report, snapshot request, etc.).

This mixed strategy is common in evolving Rust codebases, but the project lacks strict placement rules, so discoverability is uneven.

---

## 7) Type/Module Layout Cleanup Plan

### Rule Set to Adopt

1. `src/models/`
   - Persisted and externally-consumed contracts only.
2. `src/domain/<feature>/types.rs` (or feature-local equivalent)
   - Internal operational types local to one feature.
3. `src/cli/commands/*`
   - Orchestration and presentation only.

### Concrete actions

1. Resolve `detection`:
   - Either delete and migrate residual dependencies, or move to `legacy/` with deprecation note.
2. Remove duplicate parsing logic:
   - Keep one canonical batch-size parser.
3. Decouple prompt type from `detection`:
   - Move `BatchSizeMismatchChoice` to stable shared contract module if still needed.
4. Normalize lifecycle type ownership:
   - Keep persisted lifecycle definitions close to persisted state contracts.
5. Add a short `ARCHITECTURE.md` convention section:
   - Explicit placement rules + examples.

---

## 8) Practical Next Session Checklist

1. Confirm whether `detection` is intended runtime behavior or legacy only.
2. If legacy:
   - Move `BatchSizeMismatchChoice` out of `detection` (or remove feature).
   - Remove `detection` exports and update tests accordingly.
3. Add type-placement conventions to docs and enforce in review.
4. Start `AppContext` extraction in command flows to reduce orchestration complexity.

