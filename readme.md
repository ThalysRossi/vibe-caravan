# caravan

`caravan` is a Rust migration tool vibe coded for a very specific situation: On moving to Linux this year, I found that my storage hard drive is a **Storage Spaces** volume, since I'm currently (feb-2026) dual booting with win11, fully intent on migrating fully to Linux, I was faced with this issue when trying to access my data. Seeing as Linux cannot directly access Windows Storage Spaces, I need to transfer my files to regular NTFS drives which Linux can access, and then copy them from the NTFS drives into the new btrfs partition. With a data-set of over 3.5TB, doing this manually seemed like a really error prone task and one that would take a while, as the NTFS drives are way smaller than needed. This led me to create this tool to be able to automate part of the process as well as have some form of data integrity checking, resumable states and btrfs snapshots for added safety. I opted to use rust as it is a language I'm curious about.

A few of the requirements I came up with are: 
- splitting data into batches of a configurable size
- refusing to start a batch migration if the destination does not have enough free space
- two running modes: staging for getting the data from Storage Spaces into the intermediary drives, migration for getting the data from NTFS to btrfs
- copying each batch using a mode-appropriate backend
- verifying the batch after copy
- prompting for human review when verification fails
- prompting again before deleting source files
- resuming safely after interruptions
- creating Btrfs snapshots during the Linux final migration phase

## Modes

### Staging Mode

This mode is run in Windows to prepare the data transfer by creating the batches and moving them from the Storage Spaces volume to the intermediary NTFS drives.


```text
caravan staging --source D:\Data --dest E:\staging --batch-size 100GiB --interactive
```

### Migration mode

This mode is run in Linux and is for the final phase, where staged data is copied to the btrfs destination drives and the snapshots created.

```text
caravan migrate --source /mnt/staging/Data --dest /mnt/data/@media --batch-size 100GiB --snapshot-every 1 --interactive
```

## Safety model

`caravan` uses two human-review gates:

1. if verification fails, it stops and asks for review before doing anything destructive
2. if verification succeeds, it still stops and asks for approval before deleting source files

It also checks destination free space before each batch. If the available space is less than or equal to the planned batch size, the batch is aborted before copying starts.

## Graceful Shutdown & Automatic Resume

Caravan supports graceful shutdown via Ctrl+C (SIGINT on Unix, Ctrl+C on Windows). When a shutdown signal is received:

- The current batch operation completes (copy or verification finishes)
- No new batches are started
- The migration state is saved to disk
- The program exits cleanly with a `GracefulShutdown` error

### Automatic Resume Feature

When you re-run the same `staging` or `migrate` command with identical source and destination paths:

1. The tool automatically detects if an incomplete migration exists for those paths
2. It loads the existing state file from the `.caravan` directory in the source
3. Batches that are already `CopyCompleted` or `VerifyCompleted` are skipped
4. The migration continues from where it left off

This means you can simply re-run the same command after an interruption, without needing the `resume` subcommand.

### State Files & `.caravan` Directory

Caravan saves migration state to a `.caravan` directory in the source folder (e.g., `/source/.caravan/migration_abcdef12.json`). This directory is:

- Automatically created when needed
- Excluded from scanning and verification operations
- Used to store state files, migration registry, and batch definitions

### Manual Resume

You can also use the `caravan resume` command to explicitly resume from a specific state file, which is useful when:

- You need to resume from a different location
- You want to override the automatic detection
- You're troubleshooting state file issues

This feature ensures that long-running migrations can be safely interrupted without losing progress or corrupting data.

## Installation

Build from source with Cargo:

```bash
cargo build --release
```

Then run the resulting binary from `target/release/caravan`.

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
./target/release/caravan migrate --source /mnt/staging --dest /mnt/data/@target --batch-size 100GiB --snapshot-every 1 --interactive
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
caravan staging \
  --source D:\\Photos \
  --dest E:\\caravan-staging\\Photos \
  --batch-size 100GiB \
  --interactive
```

Example with smaller batches for a very cautious run:

```bash
caravan staging \
  --source D:\\Archives \
  --dest E:\\caravan-staging\\Archives \
  --batch-size 25GiB \
  --interactive
```

### `migrate`

Used on Linux to move staged data into Btrfs.

Example:

```bash
caravan migrate \
  --source /mnt/staging/Photos \
  --dest /mnt/data/@media \
  --batch-size 100GiB \
  --snapshot-every 2 \
  --interactive
```

Example for important documents with frequent snapshots:

```bash
caravan migrate \
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
caravan status --state /var/lib/caravan/state.json
```

### `resume`

Continues an interrupted run from the saved state.

Example:

```bash
caravan resume --state /var/lib/caravan/state.json
```

## Batch size guidance

A batch size of around **100GiB** is a good starting point for large HDD-based moves.

Smaller batches can be better when:

- the data is more sensitive
- the source or destination is slower
- you want more frequent checkpoints
- you want the operator to have finer-grained control

## Behavior when space is tight

Before any copy begins, `caravan` checks the destination's free space.

If the destination has **less than or equal to** the batch size available, the batch is aborted and the tool reports that the destination must be expanded or the source layout must be reduced further before continuing.
