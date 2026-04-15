use std::path::Path;

use crate::format;
use crate::models::batch::Batch;
use crate::models::state::{BatchPhase, BatchState, JournalEntry};
use crate::preflight::PreflightWarning;

pub(crate) fn print_phase_banner(title: &str) {
    println!("\n=== {title} ===");
}

pub(crate) fn print_copy_batch_banner(batch: &Batch) {
    println!(
        "\n=== Copying {} ({} files, {}) ===",
        batch.id,
        batch.file_count,
        format::format_bytes(batch.total_bytes)
    );
}

pub(crate) fn print_verify_batch_banner(batch: &Batch) {
    println!(
        "\n=== Verifying {} ({} files, {}) ===",
        batch.id,
        batch.file_count,
        format::format_bytes(batch.total_bytes)
    );
}

pub(crate) fn print_resume_processing_batch_banner(batch: &Batch) {
    println!(
        "\n=== Processing {} ({} files, {}) ===",
        batch.id,
        batch.file_count,
        format::format_bytes(batch.total_bytes)
    );
}

pub(crate) fn print_skip_already_completed(batch_id: &str) {
    println!("Skipping {}: already completed", batch_id);
}

pub(crate) fn print_resume_skip_already_completed(batch_id: &str) {
    println!("⏭️  Skipping {}: already completed", batch_id);
}

pub(crate) fn print_skip_copy_already_completed(batch_id: &str, phase: BatchPhase) {
    println!(
        "Skipping {}: copy already completed (phase: {:?})",
        batch_id, phase
    );
}

pub(crate) fn print_skip_verification_requires_operator_review(batch_id: &str) {
    println!(
        "Skipping {}: requires operator review before verification",
        batch_id
    );
}

pub(crate) fn print_skip_verification_already_completed(batch_id: &str, phase: BatchPhase) {
    println!(
        "Skipping {}: verification already completed (phase: {:?})",
        batch_id, phase
    );
}

pub(crate) fn print_verification_passed() {
    println!("Verification passed!");
}

pub(crate) fn print_resume_verification_passed() {
    println!("✅ Verification passed!");
}

pub(crate) fn print_resume_continue_to_deletion(batch_id: &str) {
    println!(
        "✅ {} already verified, will continue to deletion phase",
        batch_id
    );
}

pub(crate) fn print_state_save_locations(primary_path: &Path, secondary_path: &Path) {
    println!(
        "State will be saved to: {} (primary) and {} (backward compatibility)",
        primary_path.display(),
        secondary_path.display()
    );
}

pub(crate) fn print_plan_summary(
    batch_count: usize,
    source_file_count: usize,
    source_total_bytes: u64,
) {
    println!(
        "Planned {} batches for {} files ({} total)",
        batch_count,
        source_file_count,
        format::format_bytes(source_total_bytes)
    );
}

pub(crate) fn print_migration_complete(processed_batches: u32, completed_count: usize) {
    println!(
        "\n=== Migration complete! {} batches processed, {} total completed ===",
        processed_batches, completed_count
    );
}

pub(crate) fn print_resume_state_header() {
    println!("=== Resuming from saved state ===");
}

pub(crate) fn print_resume_state_details(
    mode: &str,
    source: &str,
    destination: &str,
    total_batches: usize,
) {
    println!("Mode: {}", mode);
    println!("Source: {}", source);
    println!("Destination: {}", destination);
    println!("Total batches: {}", total_batches);
}

pub(crate) fn print_resume_completed_batches(completed_count: usize, total_batches: usize) {
    println!("Completed batches: {} / {}", completed_count, total_batches);
}

pub(crate) fn print_resuming_transfer() {
    println!("Resuming transfer...\n");
}

pub(crate) fn print_resume_complete(total_batches: usize, completed_count: usize) {
    println!(
        "\n✅ Resume complete! {} batches processed, {} total completed",
        total_batches, completed_count
    );
}

pub(crate) fn print_status_header(mode: &str, source: &str, destination: &str, batch_count: usize) {
    println!("=== Caravan Status ===");
    println!("Mode: {}", mode);
    println!("Source: {}", source);
    println!("Destination: {}", destination);
    println!("Batches: {}", batch_count);
}

pub(crate) fn print_status_batch(batch: &BatchState) {
    println!(
        "  {} - {:?} (verified: {}, approved: {}, deleted: {})",
        batch.batch_id,
        batch.phase,
        batch.verification_passed,
        batch.approved_for_delete,
        batch.deleted
    );
}

pub(crate) fn print_last_snapshot(snapshot: &str) {
    println!("Last snapshot: {}", snapshot);
}

pub(crate) fn print_status_journal_header(entry_count: usize) {
    println!("\nJournal entries: {}", entry_count);
}

pub(crate) fn print_status_journal_entry(entry: &JournalEntry) {
    println!(
        "  [{}] {} - {} ({})",
        entry.timestamp_unix_secs, entry.event, entry.batch_id, entry.context
    );
}

pub(crate) fn print_all_verified_banner(batch_count: usize) {
    println!(
        "\n=== All {} batches have been verified successfully ===",
        batch_count
    );
}

pub(crate) fn print_deletion_not_approved() {
    println!("Deletion not approved. Stopping.");
}

pub(crate) fn print_delete_source_batches_banner(batch_count: usize) {
    println!(
        "\n=== Deleting source files for {} batch(es) ===",
        batch_count
    );
}

pub(crate) fn print_preflight_warnings(warnings: &[PreflightWarning]) {
    if warnings.is_empty() {
        return;
    }

    println!("\n=== Preflight Warnings ===");
    for warning in warnings {
        println!("  [{}] {}", warning.code.as_str(), warning.message);
    }
}
