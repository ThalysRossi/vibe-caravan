# Caravan Manual Test Plan

## Overview
This manual test plan covers the comprehensive testing of `caravan`, a Rust migration tool for Windows-to-Linux data migration. The tool operates in two modes:
- **Staging Mode**: Run on Windows to copy data from Storage Spaces to Linux-readable NTFS drives
- **Migration Mode**: Run on Linux to copy staged data to btrfs with optional snapshots

## Test Environment Requirements

### Windows Environment
- Windows 10/11 with Storage Spaces volume (or simulated with regular NTFS)
- Rust toolchain installed
- NTFS-formatted intermediary drive(s)
- Administrative privileges for file operations

### Linux Environment  
- Linux distribution with btrfs support (CachyOS, Ubuntu, etc.)
- Rust toolchain installed
- btrfs filesystem mounted with snapshot capability
- NTFS support (ntfs-3g) for reading intermediary drives

## Test Data Preparation

### Synthetic Test Data Sets
1. **Small Dataset**: 100 files, total size <1GB (quick validation)
2. **Medium Dataset**: 10,000 files, total size ~10GB (stress batching logic)
3. **Large Dataset**: 100,000+ files, total size 100GB+ (test memory usage)
4. **Mixed Dataset**: Combination of large (>1GB) and small (<1MB) files
5. **Edge Case Dataset**: Files with special characters, Unicode names, deep nesting

### Real Data Considerations
- Never test with production data without backup
- Use dedicated test partitions
- Verify no destructive operations affect real data

## Test Categories

### 1. CLI Interface & Configuration Tests

#### 1.1 Command Parsing
- [ ] `caravan staging --source /path --dest /path --batch-size 100GiB --interactive`
- [ ] `caravan migrate --source /path --dest /path --batch-size 100GiB --snapshot-every 2 --interactive`
- [ ] `caravan status`
- [ ] `caravan resume`
- [ ] `caravan --log-level debug staging ...`

#### 1.2 Argument Validation
- [ ] Invalid batch sizes (0, negative, malformed units: "abc", "10XB")
- [ ] Same source and destination paths (should fail)
- [ ] Missing required arguments (source, dest, batch-size)
- [ ] Invalid verification modes (if any beyond structural/digest/strict)
- [ ] Snapshot settings in staging mode (should fail)
- [ ] `--max-files 0` (should fail)
- [ ] `--snapshot-every 0` (should fail)

#### 1.3 Interactive Mode
- [ ] Non-interactive mode blocks destructive operations
- [ ] Interactive mode prompts for approval
- [ ] Default behavior when `--interactive` not specified

### 2. Scanning & Planning Tests

#### 2.1 Source Scanning
- [ ] Empty source directory (produces 0 batches)
- [ ] Single large file (> batch size) forms own batch
- [ ] Many small files grouped by size/file count limits
- [ ] Deep directory structure preservation
- [ ] Symlinks handling (should skip or report)
- [ ] Permission denied directories (should error gracefully)
- [ ] Source doesn't exist (should fail with clear error)
- [ ] Source is a file, not directory (should fail)

#### 2.2 Batch Planning
- [ ] Files from same directory stay together when possible
- [ ] Batch size limit respected
- [ ] File count limit (`--max-files`) respected
- [ ] Deterministic batch IDs across runs
- [ ] Large file (> batch size) creates single-file batch
- [ ] Mixed file sizes handled appropriately

### 3. Capacity Checking Tests

#### 3.1 Space Validation
- [ ] Sufficient space: proceed with copy
- [ ] Insufficient space (< batch size): abort with clear error
- [ ] Exactly batch size free: abort (safety margin)
- [ ] Reserve margin applied correctly
- [ ] Space check after partial copy (resume scenario)

#### 3.2 Error Messages
- [ ] Clear abort reason includes numbers
- [ ] Human-readable byte counts (GiB, MiB)
- [ ] Suggested actions when possible

### 4. Copy & Transfer Tests

#### 4.1 File Copy Operations
- [ ] Directory structure preserved
- [ ] File contents identical (verified later)
- [ ] Large files (>4GB) handled correctly
- [ ] Files with special characters (spaces, Unicode, symbols)
- [ ] Read-only source files
- [ ] Permission preservation (where possible across FS)

#### 4.2 Error Handling During Copy
- [ ] Destination not writable (permission denied)
- [ ] Disk full during copy
- [ ] Source file disappears mid-copy
- [ ] Network timeout (if network paths used)

### 5. Verification Tests

#### 5.1 Verification Modes
- [ ] **Structural**: File existence and size only
- [ ] **Digest**: Blake3 hash verification (default)
- [ ] **Strict**: Full verification (if implemented)

#### 5.2 Verification Scenarios
- [ ] All files match: PASS
- [ ] Missing destination file: FAIL with list
- [ ] Size mismatch: FAIL with details
- [ ] Digest mismatch: FAIL with details
- [ ] Source unreadable after copy: FAIL
- [ ] Destination unreadable: FAIL

#### 5.3 Verification Reports
- [ ] Report includes file counts
- [ ] Report includes bytes compared
- [ ] Missing files listed
- [ ] Mismatched files listed
- [ ] Recommended action clear

### 6. Approval & Deletion Tests

#### 6.1 Approval Flow
- [ ] Interactive mode prompts for each batch
- [ ] Batch group approval (all batches at once)
- [ ] "No" response stops deletion
- [ ] "Yes" response proceeds with deletion
- [ ] Approval state persisted in state file

#### 6.2 Deletion Operations
- [ ] Only source files deleted (not directories)
- [ ] Deletion idempotent (running twice safe)
- [ ] Partial deletion recovery
- [ ] Verification failure blocks deletion
- [ ] Non-interactive mode without approval blocks deletion

#### 6.3 Safety Gates
- [ ] Gate 1: Verification failure → stop, require review
- [ ] Gate 2: Verification pass → stop, require approval
- [ ] Both gates must pass before deletion

### 7. State Management & Resume Tests

#### 7.1 State File Operations
- [ ] State file created at `.caravan/state.json`
- [ ] State updated after each phase
- [ ] State contains all necessary metadata
- [ ] State file readable after crash
- [ ] Corrupted state file handled gracefully

#### 7.2 Resume Scenarios
- [ ] Interrupt during copy (Ctrl+C), then `caravan resume`
- [ ] Interrupt during verification, then resume
- [ ] Interrupt after approval, before deletion
- [ ] Resume with modified source (files added/removed)
- [ ] Resume with batch size mismatch detection

#### 7.3 State Reconciliation
- [ ] Partial copy detected and handled
- [ ] Already deleted files not re-deleted
- [ ] Verification not repeated unnecessarily
- [ ] Batch phase transitions tracked correctly

### 8. Snapshot Tests (Linux Migration Mode Only)

#### 8.1 Snapshot Creation
- [ ] Snapshots created at configured cadence (`--snapshot-every`)
- [ ] Snapshot naming includes batch ID and timestamp
- [ ] Snapshot metadata stored in state
- [ ] Snapshot failures recorded but don't block migration

#### 8.2 Platform Restrictions
- [ ] Snapshot settings rejected in staging mode
- [ ] Snapshots only in migrate mode
- [ ] btrfs-specific commands only on Linux

#### 8.3 Snapshot Verification
- [ ] Snapshots are readable
- [ ] Snapshot contains correct data state
- [ ] Multiple snapshots don't interfere

### 9. Platform-Specific Tests

#### 9.1 Windows (Staging Mode)
- [ ] Windows-style paths (`D:\Data`, `E:\staging`)
- [ ] Backslash path handling
- [ ] Drive letter handling
- [ ] Windows permissions/ACLs (as preserved)
- [ ] Windows line endings in text files

#### 9.2 Linux (Migration Mode)
- [ ] Linux-style paths (`/mnt/staging`, `/mnt/btrfs/@data`)
- [ ] Forward slash path handling
- [ ] btrfs snapshot commands work
- [ ] Linux permissions preserved
- [ ] Symlink handling

### 10. Error Handling & Edge Cases

#### 10.1 File System Errors
- [ ] Source directory doesn't exist
- [ ] Destination directory not writable
- [ ] Disk full during operation
- [ ] Permission denied at various stages
- [ ] File locked by another process

#### 10.2 Data Integrity Edge Cases
- [ ] Empty files
- [ ] Very large files (>1TB if supported)
- [ ] Files with same content, different names
- [ ] Files modified during migration
- [ ] Hard links (if present)

#### 10.3 Concurrency & Interruption
- [ ] Don't run two caravan instances on same paths
- [ ] Source files modified during migration
- [ ] System reboot during operation
- [ ] Power loss simulation

### 11. Performance & Scale Tests

#### 11.1 Memory Usage
- [ ] Large directory scanning memory consumption
- [ ] Batch planning memory usage
- [ ] State file size with many batches

#### 11.2 Throughput
- [ ] Copy speed with large files
- [ ] Copy speed with many small files
- [ ] Verification speed comparison (structural vs digest)

#### 11.3 Scalability
- [ ] 1,000+ batches planned correctly
- [ ] State file remains manageable
- [ ] Resume time with many batches

### 12. Integration & End-to-End Workflow

#### 12.1 Complete Staging → Migration Flow
```
1. Windows:
   caravan staging --source D:\Data --dest E:\staging --batch-size 50GiB --interactive
   
2. Move NTFS drive to Linux system
   
3. Linux:
   caravan migrate --source /mnt/staging --dest /mnt/btrfs/@data --batch-size 50GiB --snapshot-every 2 --interactive
```

#### 12.2 Data Integrity Verification
- [ ] Compare source (Storage Spaces) to final btrfs destination
- [ ] Use external tools: `rsync --dry-run -n` or `diff -r`
- [ ] Verify no data corruption
- [ ] Verify directory structure identical

#### 12.3 Recovery Testing
- [ ] Simulate failure at each stage
- [ ] Verify resume works correctly
- [ ] Verify no data loss in failure scenarios

## Test Execution Checklist

### Pre-Test Setup
- [ ] Backup all test data
- [ ] Set up isolated test environments
- [ ] Prepare synthetic test datasets
- [ ] Document initial state

### Test Execution
- [ ] Run tests in order of complexity
- [ ] Document each test result
- [ ] Capture logs and state files
- [ ] Note any deviations from expected behavior

### Post-Test Validation
- [ ] Verify no unintended side effects
- [ ] Clean up test artifacts
- [ ] Document lessons learned
- [ ] Update test plan with new findings

## Expected Outcomes & Pass/Fail Criteria

### Success Criteria
- All safety gates function correctly
- No data loss in normal operation
- Clear error messages for failures
- Resume works after any interruption
- Platform-specific features work as expected

### Failure Modes to Catch
- Silent data corruption
- Missing error handling
- Panics or crashes
- Incorrect state persistence
- Platform incompatibilities

## Troubleshooting Guide

### Common Issues
1. **Permission denied errors**: Check running user privileges
2. **Batch size mismatches**: Verify state file vs CLI arguments
3. **Resume failures**: Check state file corruption
4. **Snapshot failures**: Verify btrfs support and permissions
5. **Verification failures**: Check filesystem compatibility issues

### Debug Commands
```bash
# View state file
cat .caravan/state.json | jq .

# Test verification manually
caravan migrate --source /test --dest /test2 --batch-size 1GiB --verification digest

# Check logs with increased verbosity
RUST_LOG=debug caravan staging ...
```

## Test Reporting Template

```
Test Case: [ID] - [Description]
Environment: [Windows/Linux] - [Details]
Date: [YYYY-MM-DD]
Tester: [Name]

Preconditions:
- [List setup steps]

Test Steps:
1. [Step 1]
2. [Step 2]
...

Expected Results:
- [Expected outcome 1]
- [Expected outcome 2]

Actual Results:
- [Actual outcome 1]
- [Actual outcome 2]

Status: [PASS/FAIL/BLOCKED]
Notes: [Any observations]
Logs: [Reference to log files]
```

## Continuous Testing Recommendations

1. **Automated Unit Tests**: Already in place (cargo test)
2. **Integration Tests**: Expand `tests/integration_smoke.rs`
3. **Fuzz Testing**: Consider for path handling and batch size parsing
4. **Cross-Platform CI**: Test on both Windows and Linux runners
5. **Performance Regression**: Monitor copy/verification speeds

## Security Considerations

- Never run with elevated privileges unnecessarily
- Validate all paths before operations
- Sanitize user input (CLI arguments)
- Secure state file (contains metadata about data)
- Consider encryption for sensitive metadata

## Appendix A: Test Data Generation Scripts

```bash
# Generate test dataset
mkdir -p test_data
# Create files of various sizes
for i in {1..100}; do
    dd if=/dev/urandom of="test_data/file_${i}.bin" bs=1M count=$((RANDOM % 10 + 1))
done
```

## Appendix B: Platform-Specific Notes

### Windows Testing
- Use PowerShell for scripting
- Consider Windows Subsystem for Linux (WSL) for hybrid testing
- Test with actual Storage Spaces if available

### Linux Testing
- Use tmpfs for fast I/O tests
- Mock btrfs commands if snapshots unavailable
- Test with different mount options

---

**Last Updated**: $(date +%Y-%m-%d)
**Test Plan Version**: 1.0
**Caravan Version**: $(caravan --version)
