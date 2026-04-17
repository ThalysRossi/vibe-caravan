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
    pub volume_free_bytes: u64,
    pub planned_batch_bytes: u64,
    pub reserve_margin_bytes: u64,
    pub probe_backend: &'static str,
    pub decision: CapacityDecision,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpaceInfo {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub volume_free_bytes: u64,
}

pub trait SpaceProbe {
    fn probe(&self, destination: &Path) -> Result<SpaceInfo, CaravanError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemSpaceProbe;

#[cfg(windows)]
const SYSTEM_SPACE_PROBE_BACKEND: &str = "win32_getdiskfreespaceexw";
#[cfg(not(windows))]
const SYSTEM_SPACE_PROBE_BACKEND: &str = "fs2";
const CUSTOM_SPACE_PROBE_BACKEND: &str = "custom_probe";

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
        return Err(CaravanError::IoContext {
            context: format!(
                "failed to read destination free space at {}",
                destination.display()
            ),
            source: std::io::Error::last_os_error(),
        });
    }

    Ok(SpaceInfo {
        total_bytes: total_number_of_bytes,
        available_bytes: free_bytes_available,
        volume_free_bytes: total_number_of_free_bytes,
    })
}

#[cfg(not(windows))]
fn query_space_info(destination: &Path) -> Result<SpaceInfo, CaravanError> {
    let total_bytes = fs2::total_space(destination).map_err(|source| {
        CaravanError::IoContext {
            context: format!(
                "failed to read destination total capacity at {}",
                destination.display()
            ),
            source,
        }
    })?;
    let available_bytes = fs2::available_space(destination).map_err(|source| {
        CaravanError::IoContext {
            context: format!(
                "failed to read destination free space at {}",
                destination.display()
            ),
            source,
        }
    })?;
    let volume_free_bytes = match fs2::free_space(destination) {
        Ok(bytes) => bytes,
        Err(_) => available_bytes,
    };

    Ok(SpaceInfo {
        total_bytes,
        available_bytes,
        volume_free_bytes,
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
                    fs::create_dir_all(dest).map_err(|source| {
                        CaravanError::IoContext {
                            context: format!(
                                "failed to create destination directory {}",
                                dest.display()
                            ),
                            source,
                        }
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
                    fs::create_dir_all(dest).map_err(|source| {
                        CaravanError::IoContext {
                            context: format!(
                                "failed to create destination directory {}",
                                dest.display()
                            ),
                            source,
                        }
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
    check_capacity_with_probe_and_backend(
        destination,
        planned_batch_bytes,
        reserve_margin_bytes,
        &probe,
        SYSTEM_SPACE_PROBE_BACKEND,
    )
}

pub fn check_capacity_with_probe(
    destination: &Path,
    planned_batch_bytes: u64,
    reserve_margin_bytes: u64,
    probe: &dyn SpaceProbe,
) -> Result<CapacityReport, CaravanError> {
    check_capacity_with_probe_and_backend(
        destination,
        planned_batch_bytes,
        reserve_margin_bytes,
        probe,
        CUSTOM_SPACE_PROBE_BACKEND,
    )
}

fn check_capacity_with_probe_and_backend(
    destination: &Path,
    planned_batch_bytes: u64,
    reserve_margin_bytes: u64,
    probe: &dyn SpaceProbe,
    probe_backend: &'static str,
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
        volume_free_bytes: space.volume_free_bytes,
        planned_batch_bytes,
        reserve_margin_bytes,
        probe_backend,
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
    let destination_resolved = match fs::canonicalize(destination) {
        Ok(path) => path.display().to_string(),
        Err(_) => destination.display().to_string(),
    };
    let required_raw_bytes = report
        .planned_batch_bytes
        .saturating_add(report.reserve_margin_bytes);
    let available_minus_reserve_raw_bytes = report
        .available_free_bytes
        .saturating_sub(report.reserve_margin_bytes);
    let headroom_raw_bytes = report.available_free_bytes as i128 - required_raw_bytes as i128;
    let decision_rule = "available_raw_bytes > required_raw_bytes";
    let decision = match report.decision {
        CapacityDecision::Proceed => "proceed",
        CapacityDecision::Abort => "abort",
    };
    let decision_reason = match report.decision {
        CapacityDecision::Proceed => format!(
            "available_raw_bytes({}) > required_raw_bytes({})",
            report.available_free_bytes, required_raw_bytes
        ),
        CapacityDecision::Abort => format!(
            "available_raw_bytes({}) <= required_raw_bytes({})",
            report.available_free_bytes, required_raw_bytes
        ),
    };

    format!(
        "destination={} destination_resolved={} volume_root={} probe_backend={} total_capacity_raw_bytes={} available_raw_bytes={} volume_free_raw_bytes={} available_minus_reserve_raw_bytes={} required_raw_bytes={} planned_raw_bytes={} reserve_raw_bytes={} headroom_raw_bytes={} decision_rule={} decision_reason=\"{}\" decision={}",
        destination.display(),
        destination_resolved,
        destination_volume_root(destination),
        report.probe_backend,
        report.total_capacity_bytes,
        report.available_free_bytes,
        report.volume_free_bytes,
        available_minus_reserve_raw_bytes,
        required_raw_bytes,
        report.planned_batch_bytes,
        report.reserve_margin_bytes,
        headroom_raw_bytes,
        decision_rule,
        decision_reason,
        decision
    )
}
