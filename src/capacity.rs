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

#[cfg(windows)]
fn query_space_info(destination: &Path) -> Result<SpaceInfo, CaravanError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide_path: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut free_bytes_available = 0u64;
    let mut total_number_of_bytes = 0u64;
    let mut total_number_of_free_bytes = 0u64;

    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide_path.as_ptr(),
            &mut free_bytes_available,
            &mut total_number_of_bytes,
            &mut total_number_of_free_bytes,
        )
    };

    if ok == 0 {
        return Err(CaravanError::InvalidArguments(format!(
            "failed to read destination free space at {}: {}",
            destination.display(),
            std::io::Error::last_os_error()
        )));
    }

    Ok(SpaceInfo {
        total_bytes: total_number_of_bytes,
        available_bytes: free_bytes_available,
    })
}

#[cfg(not(windows))]
fn query_space_info(destination: &Path) -> Result<SpaceInfo, CaravanError> {
    let total_bytes = fs2::total_space(destination).map_err(|err| {
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

        query_space_info(destination)
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

fn destination_volume_root(destination: &Path) -> String {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};

        let mut components = destination.components();
        if let Some(Component::Prefix(prefix_component)) = components.next() {
            let prefix = prefix_component.kind();
            let root = match prefix {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    format!("{}:\\", (letter as char).to_ascii_uppercase())
                }
                _ => prefix_component.as_os_str().to_string_lossy().to_string(),
            };
            return root;
        }
    }

    #[cfg(not(windows))]
    {
        if destination.is_absolute() {
            "/".to_string()
        } else {
            ".".to_string()
        }
    }
}

pub fn format_capacity_decision_trace(destination: &Path, report: &CapacityReport) -> String {
    let required_raw_bytes = report
        .planned_batch_bytes
        .saturating_add(report.reserve_margin_bytes);
    let decision = match report.decision {
        CapacityDecision::Proceed => "proceed",
        CapacityDecision::Abort => "abort",
    };

    format!(
        "destination={} volume_root={} available_raw_bytes={} required_raw_bytes={} planned_raw_bytes={} reserve_raw_bytes={} decision={}",
        destination.display(),
        destination_volume_root(destination),
        report.available_free_bytes,
        required_raw_bytes,
        report.planned_batch_bytes,
        report.reserve_margin_bytes,
        decision
    )
}
