# wololo

`wololo` is a Rust migration tool for a very specific but common painful situation:

- The original data lives on a Windows **Storage Spaces** volume.
- The machine is dual-boot, so Windows and Linux are not running at the same time.
- Linux cannot directly use the Storage Spaces volume for the final migration.
- The data must first be staged in Windows onto one or more volumes that Linux can read later.
- After rebooting into Linux, the data is migrated in controlled batches into a Btrfs destination with verification, human approval gates, resumable state, and optional snapshots.

The purpose of `wololo` is to make both halves of that workflow safer.

## What problem it solves

Large datasets are risky to move by hand, especially when the source is fragmented across multiple folders and the destination is tight on space. `wololo` is designed to reduce that risk by doing all of the following:

- splitting data into batches of a configurable size
- refusing to start a batch if the destination does not have enough free space
- copying each batch using a mode-appropriate backend
- verifying the batch after copy
- stopping for human review when verification fails
- stopping again before deleting source files
- resuming safely after interruptions
- creating Btrfs snapshots during the Linux final migration phase

## Two modes

### Staging mode

Use this mode in Windows.

It is intended for the first phase of the migration, where files are copied out of the Storage Spaces volume into a Linux-readable intermediate volume.

Typical use:

```text
wololo staging --source D:\Data --dest E:\staging --batch-size 100GiB --interactive
```

Staging mode is Windows-focused and does not use Btrfs snapshots.

### Migration mode

Use this mode in Linux.

It is intended for the final phase, where staged data is copied into the Btrfs destination.

Typical use:

```text
wololo migrate --source /mnt/staging/Data --dest /mnt/data/@media --batch-size 100GiB --snapshot-every 1 --interactive
```

Migration mode can create Btrfs snapshots after approved batches.

## Safety model

`wololo` uses two human-review gates:

1. if verification fails, it stops and asks for review before doing anything destructive
2. if verification succeeds, it still stops and asks for approval before deleting source files

It also checks destination free space before each batch. If the available space is less than or equal to the planned batch size, the batch is aborted before copying starts.

## Installation

Build from source with Cargo:

```bash
cargo build --release
```

Then run the resulting binary from `target/release/wololo`.

## Build and run

If you are working from this repository directly, use Cargo commands from the project root.

Build a debug binary:

```bash
cargo build
```

Run the tool with a subcommand:

```bash
cargo run -- staging --source /path/to/source --dest /path/to/dest --batch-size 25GiB --interactive
```

Build an optimized release binary:

```bash
cargo build --release
```

Run the release binary directly:

```bash
./target/release/wololo migrate --source /mnt/staging --dest /mnt/data/@target --batch-size 100GiB --snapshot-every 1 --interactive
```

Run all tests:

```bash
cargo test
```

## Commands

### `staging`

Used on Windows to copy from the Storage Spaces source to a Linux-readable intermediate volume.

Example:

```bash
wololo staging \
  --source D:\\Photos \
  --dest E:\\wololo-staging\\Photos \
  --batch-size 100GiB \
  --interactive
```

Example with smaller batches for a very cautious run:

```bash
wololo staging \
  --source D:\\Archives \
  --dest E:\\wololo-staging\\Archives \
  --batch-size 25GiB \
  --interactive
```

### `migrate`

Used on Linux to move staged data into Btrfs.

Example:

```bash
wololo migrate \
  --source /mnt/staging/Photos \
  --dest /mnt/data/@media \
  --batch-size 100GiB \
  --snapshot-every 2 \
  --interactive
```

Example for important documents with frequent snapshots:

```bash
wololo migrate \
  --source /mnt/staging/Documents \
  --dest /mnt/data/@documents \
  --batch-size 10GiB \
  --snapshot-every 1 \
  --interactive
```

### `status`

Shows the current saved state and progress.

Example:

```bash
wololo status --state /var/lib/wololo/state.json
```

### `resume`

Continues an interrupted run from the saved state.

Example:

```bash
wololo resume --state /var/lib/wololo/state.json
```

## Batch size guidance

A batch size of around **100GiB** is a good starting point for large HDD-based moves.

Smaller batches can be better when:

- the data is more sensitive
- the source or destination is slower
- you want more frequent checkpoints
- you want the operator to have finer-grained control

## Behavior when space is tight

Before any copy begins, `wololo` checks the destination's free space.

If the destination has **less than or equal to** the batch size available, the batch is aborted and the tool reports that the destination must be expanded or the source layout must be reduced further before continuing.

## Terminal-friendly manual

A more compact command reference is available in `manual.md`.

You can open it with:

```bash
less manual.md
```

## Suggested usage flow

1. Boot Windows.
2. Run `wololo staging` until enough data has been moved into the intermediate volume.
3. Reboot into Linux.
4. Mount the staged volume.
5. Run `wololo migrate` against the staged data.
6. Review verification reports carefully.
7. Approve deletions only when you are comfortable doing so.
8. Let `wololo` snapshot the Btrfs destination according to your configured cadence.

## Design notes

- The core engine is path-based and reusable across platforms.
- Platform-specific copy backends are used underneath.
- Btrfs snapshots are Linux-only.
- The state file is shared across resume operations.
- The manifest is the single source of truth for batch order and routing history.
- The tool is designed to stop rather than guess when something looks wrong.

