use std::fs;
use std::path::Path;

use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::verification::{DigestModeUsed, VerificationReport, VerificationStatus};
use crate::progress::ProgressReporter;

/// Check if a path should be skipped because it's inside a .caravan directory
fn should_skip_caravan(path: &Path) -> bool {
    path.components().any(|comp| {
        if let std::path::Component::Normal(name) = comp {
            name == ".caravan"
        } else {
            false
        }
    })
}

pub fn verify_batch(
    batch: &Batch,
    source_root: &Path,
    destination_root: &Path,
) -> Result<VerificationReport, CaravanError> {
    let mut no_interrupt = || Ok(());
    verify_batch_with_progress(
        batch,
        source_root,
        destination_root,
        &mut crate::progress::NoopProgress,
        &mut no_interrupt,
    )
}

pub fn verify_batch_with_progress(
    batch: &Batch,
    source_root: &Path,
    destination_root: &Path,
    progress: &mut dyn ProgressReporter,
    check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
) -> Result<VerificationReport, CaravanError> {
    let mut missing_files = Vec::new();
    let mut mismatched_files = Vec::new();
    let mut unreadable_files = Vec::new();
    let mut bytes_compared = 0_u64;

    progress.start(batch.files.len(), "Verifying");

    for (index, entry) in batch.files.iter().enumerate() {
        check_interrupt()?;

        let source_path = source_root.join(&entry.relative_path);
        let destination_path = destination_root.join(&entry.relative_path);
        let rel = entry.relative_path.to_string_lossy().to_string();

        // Skip files inside .caravan directories
        if should_skip_caravan(&entry.relative_path) {
            continue;
        }

        if !destination_path.exists() {
            missing_files.push(rel);
            continue;
        }

        let source_meta = match fs::metadata(&source_path) {
            Ok(meta) => meta,
            Err(_) => {
                unreadable_files.push(rel.clone());
                continue;
            }
        };
        let destination_meta = match fs::metadata(&destination_path) {
            Ok(meta) => meta,
            Err(_) => {
                unreadable_files.push(rel.clone());
                continue;
            }
        };

        bytes_compared = bytes_compared.saturating_add(source_meta.len());
        if source_meta.len() != destination_meta.len() {
            mismatched_files.push(rel);
            continue;
        }

        let source_digest = match digest_file(&source_path) {
            Ok(value) => value,
            Err(_) => {
                unreadable_files.push(rel.clone());
                continue;
            }
        };
        let destination_digest = match digest_file(&destination_path) {
            Ok(value) => value,
            Err(_) => {
                unreadable_files.push(rel.clone());
                continue;
            }
        };
        if source_digest != destination_digest {
            mismatched_files.push(rel);
        }

        progress.advance(index + 1, Some(&entry.relative_path.to_string_lossy()));
    }

    progress.finish();

    let digest_mode_used = DigestModeUsed::Blake3;

    let status =
        if missing_files.is_empty() && mismatched_files.is_empty() && unreadable_files.is_empty() {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        };

    let recommended_action = if status == VerificationStatus::Pass {
        "Verification passed; approval gate required before deletion.".to_string()
    } else {
        "Verification failed; stop and require human review before deletion.".to_string()
    };

    Ok(VerificationReport {
        file_count: batch.file_count,
        bytes_compared,
        missing_files,
        mismatched_files,
        unreadable_files,
        digest_mode_used,
        status,
        recommended_action,
    })
}

pub fn digest_file(path: &Path) -> Result<[u8; 32], CaravanError> {
    use std::io::Read;

    let mut file = fs::File::open(path)
        .map_err(|err| CaravanError::Io(format!("failed to open {}: {}", path.display(), err)))?;

    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; 1024 * 1024]; // 1MB streaming buffer on heap

    loop {
        let bytes_read = file.read(&mut buffer).map_err(|err| {
            CaravanError::Io(format!("failed to read {}: {}", path.display(), err))
        })?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize().into())
}
