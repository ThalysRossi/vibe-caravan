use std::fs;
use std::path::Path;

use crate::config::VerificationMode;
use crate::error::CaravanError;
use crate::models::batch::Batch;
use crate::models::verification::{DigestModeUsed, VerificationReport, VerificationStatus};

pub fn verify_batch(
    batch: &Batch,
    source_root: &Path,
    destination_root: &Path,
    mode: VerificationMode,
) -> Result<VerificationReport, CaravanError> {
    let mut missing_files = Vec::new();
    let mut mismatched_files = Vec::new();
    let mut unreadable_files = Vec::new();
    let mut bytes_compared = 0_u64;

    for entry in &batch.files {
        let source_path = source_root.join(&entry.relative_path);
        let destination_path = destination_root.join(&entry.relative_path);
        let rel = entry.relative_path.to_string_lossy().to_string();

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

        if mode != VerificationMode::Structural {
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
        }
    }

    let digest_mode_used = if mode == VerificationMode::Structural {
        DigestModeUsed::None
    } else {
        DigestModeUsed::Blake3
    };

    let status = if missing_files.is_empty() && mismatched_files.is_empty() && unreadable_files.is_empty() {
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

fn digest_file(path: &Path) -> Result<[u8; 32], CaravanError> {
    let bytes = fs::read(path).map_err(|err| {
        CaravanError::InvalidArguments(format!("failed to read {}: {err}", path.display()))
    })?;
    Ok(blake3::hash(&bytes).into())
}
