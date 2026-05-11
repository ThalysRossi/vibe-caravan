use std::collections::VecDeque;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestinationSpaceDiagnostics {
    pub destination_exists: bool,
    pub visible_file_count: u64,
    pub visible_logical_bytes: u64,
    pub caravan_part_file_count: u64,
    pub caravan_part_logical_bytes: u64,
    pub scan_error_count: u64,
}

pub trait SpaceProbe {
    fn probe(&self, destination: &Path) -> Result<SpaceInfo, CaravanError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemSpaceProbe;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux::{destination_volume_root, query_space_info, system_space_probe_backend};
#[cfg(target_os = "windows")]
use windows::{destination_volume_root, query_space_info, system_space_probe_backend};

const CUSTOM_SPACE_PROBE_BACKEND: &str = "custom_probe";
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
                    fs::create_dir_all(dest).map_err(|source| CaravanError::IoContext {
                        context: format!(
                            "failed to create destination directory {}",
                            dest.display()
                        ),
                        source,
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
                    fs::create_dir_all(dest).map_err(|source| CaravanError::IoContext {
                        context: format!(
                            "failed to create destination directory {}",
                            dest.display()
                        ),
                        source,
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
        system_space_probe_backend(),
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

pub fn inspect_destination_space(destination: &Path) -> DestinationSpaceDiagnostics {
    let mut diagnostics = DestinationSpaceDiagnostics {
        destination_exists: destination.exists(),
        visible_file_count: 0,
        visible_logical_bytes: 0,
        caravan_part_file_count: 0,
        caravan_part_logical_bytes: 0,
        scan_error_count: 0,
    };

    if !diagnostics.destination_exists {
        return diagnostics;
    }

    let mut pending = VecDeque::from([destination.to_path_buf()]);
    while let Some(path) = pending.pop_front() {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                diagnostics.scan_error_count += 1;
                continue;
            }
        };
        let file_type = metadata.file_type();

        if file_type.is_file() {
            diagnostics.visible_file_count += 1;
            diagnostics.visible_logical_bytes = diagnostics
                .visible_logical_bytes
                .saturating_add(metadata.len());

            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".caravan.part"))
            {
                diagnostics.caravan_part_file_count += 1;
                diagnostics.caravan_part_logical_bytes = diagnostics
                    .caravan_part_logical_bytes
                    .saturating_add(metadata.len());
            }
            continue;
        }

        if file_type.is_dir() {
            let entries = match fs::read_dir(&path) {
                Ok(entries) => entries,
                Err(_) => {
                    diagnostics.scan_error_count += 1;
                    continue;
                }
            };

            for entry in entries {
                match entry {
                    Ok(entry) => pending.push_back(entry.path()),
                    Err(_) => diagnostics.scan_error_count += 1,
                }
            }
        }
    }

    diagnostics
}

pub fn format_destination_space_diagnostic(
    destination: &Path,
    report: &CapacityReport,
    diagnostics: &DestinationSpaceDiagnostics,
) -> String {
    let volume_used_raw_bytes = report
        .total_capacity_bytes
        .saturating_sub(report.volume_free_bytes);
    let hidden_or_other_raw_bytes =
        volume_used_raw_bytes.saturating_sub(diagnostics.visible_logical_bytes);
    let hidden_hint = if hidden_or_other_raw_bytes
        > report.planned_batch_bytes.max(1024 * 1024 * 1024)
    {
        format!(
            " hidden_space_hint=\"volume used greatly exceeds files visible under destination; on Windows run elevated: fsutil volume allocationreport {}; fsutil usn queryjournal {}\"",
            destination_volume_root(destination),
            destination_volume_root(destination)
        )
    } else {
        String::new()
    };

    format!(
        "destination_usage destination={} destination_exists={} visible_file_count={} visible_logical_raw_bytes={} caravan_part_file_count={} caravan_part_logical_raw_bytes={} scan_error_count={} volume_used_raw_bytes={} volume_free_raw_bytes={} hidden_or_other_raw_bytes={}{}",
        destination.display(),
        diagnostics.destination_exists,
        diagnostics.visible_file_count,
        diagnostics.visible_logical_bytes,
        diagnostics.caravan_part_file_count,
        diagnostics.caravan_part_logical_bytes,
        diagnostics.scan_error_count,
        volume_used_raw_bytes,
        report.volume_free_bytes,
        hidden_or_other_raw_bytes,
        hidden_hint
    )
}
