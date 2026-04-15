use std::fs;
use std::path::Path;

use crate::error::CaravanError;
use crate::format::format_bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapacityDecision {
    Proceed,
    Abort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapacityReport {
    pub total_capacity_bytes: u64,
    pub available_free_bytes: u64,
    pub planned_batch_bytes: u64,
    pub reserve_margin_bytes: u64,
    pub decision: CapacityDecision,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpaceInfo {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

pub trait SpaceProbe {
    fn probe(&self, destination: &Path) -> Result<SpaceInfo, CaravanError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemSpaceProbe;

impl SpaceProbe for SystemSpaceProbe {
    fn probe(&self, destination: &Path) -> Result<SpaceInfo, CaravanError> {
        // Helper to check if we should create the directory
        fn ensure_destination_exists(dest: &Path) -> Result<(), CaravanError> {
            if dest.exists() {
                return Ok(());
            }

            // Check if parent exists
            let parent = dest.parent();
            match parent {
                Some(p) if p.exists() => {
                    // Top-level directory (parent exists) - create it
                    println!("Creating destination directory: {}", dest.display());
                    fs::create_dir_all(dest).map_err(|err| {
                        CaravanError::InvalidArguments(format!(
                            "failed to create destination directory {}: {err}",
                            dest.display()
                        ))
                    })?;
                    Ok(())
                }
                Some(p) => {
                    // Subdirectory where parent doesn't exist - fail
                    Err(CaravanError::InvalidArguments(format!(
                        "destination directory {} does not exist and cannot be created because parent directory {} does not exist",
                        dest.display(),
                        p.display()
                    )))
                }
                None => {
                    // No parent (root-like path) - shouldn't happen but try to create
                    println!("Creating destination directory: {}", dest.display());
                    fs::create_dir_all(dest).map_err(|err| {
                        CaravanError::InvalidArguments(format!(
                            "failed to create destination directory {}: {err}",
                            dest.display()
                        ))
                    })?;
                    Ok(())
                }
            }
        }

        // Ensure destination exists before checking capacity
        ensure_destination_exists(destination)?;

        let total_bytes = fs2::total_space(destination).map_err(|err| {
            // Improve error message for missing directory vs capacity issues
            if err.kind() == std::io::ErrorKind::NotFound {
                CaravanError::InvalidArguments(format!(
                    "destination directory {} does not exist and could not be created",
                    destination.display()
                ))
            } else {
                CaravanError::InvalidArguments(format!(
                    "failed to read destination total capacity at {}: {err}",
                    destination.display()
                ))
            }
        })?;
        let available_bytes = fs2::available_space(destination).map_err(|err| {
            CaravanError::InvalidArguments(format!(
                "failed to read destination free space at {}: {err}",
                destination.display()
            ))
        })?;

        Ok(SpaceInfo {
            total_bytes,
            available_bytes,
        })
    }
}

pub fn check_capacity(
    destination: &Path,
    planned_batch_bytes: u64,
    reserve_margin_bytes: u64,
) -> Result<CapacityReport, CaravanError> {
    let probe = SystemSpaceProbe;
    check_capacity_with_probe(
        destination,
        planned_batch_bytes,
        reserve_margin_bytes,
        &probe,
    )
}

pub fn check_capacity_with_probe(
    destination: &Path,
    planned_batch_bytes: u64,
    reserve_margin_bytes: u64,
    probe: &dyn SpaceProbe,
) -> Result<CapacityReport, CaravanError> {
    if planned_batch_bytes == 0 {
        return Err(CaravanError::InvalidArguments(
            "planned batch size must be greater than zero".to_string(),
        ));
    }

    let space = probe.probe(destination)?;
    let required_bytes = planned_batch_bytes.saturating_add(reserve_margin_bytes);
    let decision = if space.available_bytes > required_bytes {
        CapacityDecision::Proceed
    } else {
        CapacityDecision::Abort
    };

    let reason = if decision == CapacityDecision::Abort {
        Some(format!(
            "insufficient destination space: available={} required={} (batch={} reserve={})",
            format_bytes(space.available_bytes),
            format_bytes(required_bytes),
            format_bytes(planned_batch_bytes),
            format_bytes(reserve_margin_bytes)
        ))
    } else {
        None
    };

    Ok(CapacityReport {
        total_capacity_bytes: space.total_bytes,
        available_free_bytes: space.available_bytes,
        planned_batch_bytes,
        reserve_margin_bytes,
        decision,
        reason,
    })
}
