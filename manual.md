# Caravan Operator Manual

## Workflow

`caravan` is designed for a two-step migration flow:

1. Run `staging` on Windows to copy from Storage Spaces to NTFS staging drives.
2. Run `migrate` on Linux to copy from NTFS staging to Btrfs targets.

Each run follows the same high-level sequence:

1. build deterministic plan and batches
2. preflight safety checks (capacity and filesystem guardrails)
3. copy phase
4. verification phase
5. deletion gate (explicit operator approval)
6. optional snapshot phase (migrate mode only)

Use `status` to inspect progress and `resume` to continue from saved state.

## Installation and PATH

Linux user-local install:

```bash
cargo build --release
install -Dm755 ./target/release/caravan ~/.local/bin/caravan
```

Windows user-local install (PowerShell):

```powershell
cargo build --release
$BinDir = "$HOME\bin"
New-Item -ItemType Directory -Force $BinDir | Out-Null
Copy-Item .\target\release\caravan.exe "$BinDir\caravan.exe" -Force
```

Both environments require the install directory to be present in `PATH`.

## Interactive vs Scriptable

Interactive operation is recommended for normal runs:

- add `--interactive` for explicit human approval before deletion
- allows human review at safety gates

Scriptable operation is intended for controlled automation:

- omit `--interactive` for fail-closed non-destructive behavior
- destructive steps remain blocked without approval artifacts
- use `resume --recover-failed` only when you explicitly intend failed-batch recovery

Conflict behavior is policy-driven:

- `--conflict-policy skip-file` (default): copy non-conflicting files, keep conflicting files untouched, then require operator review
- `--conflict-policy skip-batch`: skip whole batch on any conflict

## Safety Gates

Caravan has explicit destructive-operation gates:

1. Verification gate:
If verification fails, source deletion is blocked.

2. Deletion approval gate:
Even after successful verification, source deletion requires approval.

3. Conflict gate:
Conflict handling never auto-overwrites destination conflicts under default policy.

4. Capacity gate:
If destination free space is less than or equal to planned batch size, copy is blocked before transfer.

## Safe Defaults

- conflict policy default: `skip-file`
- copy strategy default: `auto`
- `native` strategy: supported on Windows, rejected on Linux
- non-interactive behavior: fail closed for destructive operations

## Minimal Command Examples

Staging:

```bash
caravan staging --source D:\Data --dest E:\staging --batch-size 100GiB --interactive
```

Migration:

```bash
caravan migrate --source /mnt/staging/Data --dest /mnt/data/@media --batch-size 100GiB --snapshot-every 1 --interactive
```

Inspect and resume:

```bash
caravan status
caravan resume --state .caravan/state.json
```

## Linux Man Page

View the bundled man page without installing:

```bash
man -l docs/man/caravan.1
```

Install for normal `man caravan` usage:

```bash
bash scripts/install_manpage.sh
MANPATH="$HOME/.local/share/man${MANPATH:+:$MANPATH}" man caravan
```
