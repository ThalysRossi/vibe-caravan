# Caravan Architecture Conventions

This document defines type/module placement rules used by this repository.

## Module Placement Rules

1. `src/models/`
Contains persisted and externally-consumed contracts.
Examples:
- `MigrationState`
- `BatchState`
- `PlannedBatch`

2. Feature modules (`src/<feature>.rs`, `src/cli/commands/<feature>/`)
Contain operational logic local to that feature.
Examples:
- preflight checks
- transfer orchestration
- resume planning

3. CLI command modules (`src/cli/commands/*`)
Contain orchestration and rendering concerns only.
They should compose domain functions and avoid embedding low-level filesystem behavior directly.

4. Shared parsing/utility contracts
Live in dedicated shared modules when reused by multiple features.
Examples:
- `src/size.rs` for human size parsing
- `src/prompt.rs` contracts for interactive operator choices

## Refactor Guardrails

1. Keep persisted state format backward compatible by default.
2. When moving types, prefer internal rewiring plus stable external behavior.
3. Add tests before refactors that change module ownership or behavior.

## Conditional Compilation Policy

1. In `src`, allowed compile-time predicates are:
- `#[cfg(target_os = "windows")]`
- `#[cfg(target_os = "linux")]`

2. Disallowed in `src`:
- `#[cfg(unix)]`
- `#[cfg(not(unix))]`
- `#[cfg(windows)]`
- `#[cfg(not(windows))]`
- any `#[cfg(not(...))]` predicate for platform routing

3. cfg!(...) runtime checks must be centralized in src/platform.rs.

4. Keep platform specialization at module boundaries for hotspot modules:
- `src/scan/{mod.rs,windows.rs,linux.rs}`
- `src/preflight/{mod.rs,platform_windows.rs,platform_linux.rs}`
- `src/capacity/{mod.rs,windows.rs,linux.rs}`

5. Run policy checks with:
- `bash scripts/check_cfg_policy.sh`
