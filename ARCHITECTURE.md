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
