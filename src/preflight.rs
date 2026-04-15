use std::collections::HashMap;
use std::path::Path;

use crate::capacity::{SpaceProbe, SystemSpaceProbe};
use crate::config::{Mode, TransferConfig};
use crate::error::CaravanError;
use crate::plan::PlanningSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DestinationFlags {
    pub is_compressed: bool,
    pub is_reparse_point: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DestinationSpaceSnapshot {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub volume_free_bytes: u64,
}

pub trait DestinationProbe {
    fn destination_flags(&self, destination: &Path) -> Result<DestinationFlags, CaravanError>;

    fn destination_space(
        &self,
        _destination: &Path,
    ) -> Result<Option<DestinationSpaceSnapshot>, CaravanError> {
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemDestinationProbe;

impl DestinationProbe for SystemDestinationProbe {
    fn destination_flags(&self, destination: &Path) -> Result<DestinationFlags, CaravanError> {
        query_destination_flags(destination)
    }

    fn destination_space(
        &self,
        destination: &Path,
    ) -> Result<Option<DestinationSpaceSnapshot>, CaravanError> {
        let probe = SystemSpaceProbe;
        let space = probe.probe(destination)?;
        Ok(Some(DestinationSpaceSnapshot {
            total_bytes: space.total_bytes,
            available_bytes: space.available_bytes,
            volume_free_bytes: space.volume_free_bytes,
        }))
    }
}

#[cfg(windows)]
fn query_destination_flags(destination: &Path) -> Result<DestinationFlags, CaravanError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileAttributesW, FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_REPARSE_POINT,
        INVALID_FILE_ATTRIBUTES,
    };

    let wide_path: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let attrs = unsafe { GetFileAttributesW(wide_path.as_ptr()) };
    if attrs == INVALID_FILE_ATTRIBUTES {
        return Err(CaravanError::InvalidArguments(format!(
            "failed to inspect destination attributes at {}: {}",
            destination.display(),
            std::io::Error::last_os_error()
        )));
    }

    Ok(DestinationFlags {
        is_compressed: attrs & FILE_ATTRIBUTE_COMPRESSED != 0,
        is_reparse_point: attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0,
    })
}

#[cfg(not(windows))]
fn query_destination_flags(_destination: &Path) -> Result<DestinationFlags, CaravanError> {
    Ok(DestinationFlags {
        is_compressed: false,
        is_reparse_point: false,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightWarningCode {
    DestinationCompressed,
    DestinationReparsePoint,
    PathLengthPressure,
    CaseCollisionRisk,
    SpaceAccountingDivergence,
}

impl PreflightWarningCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            PreflightWarningCode::DestinationCompressed => "destination_compressed",
            PreflightWarningCode::DestinationReparsePoint => "destination_reparse_point",
            PreflightWarningCode::PathLengthPressure => "path_length_pressure",
            PreflightWarningCode::CaseCollisionRisk => "case_collision_risk",
            PreflightWarningCode::SpaceAccountingDivergence => "space_accounting_divergence",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightWarning {
    pub code: PreflightWarningCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightReport {
    pub warnings: Vec<PreflightWarning>,
    pub max_estimated_destination_path_len: usize,
    pub case_collision_count: usize,
}

fn destination_looks_windows_style(path: &Path) -> bool {
    let rendered = path.to_string_lossy();
    rendered.contains(":\\") || rendered.starts_with("\\\\")
}

fn destination_uses_extended_windows_prefix(path: &Path) -> bool {
    path.to_string_lossy().starts_with("\\\\?\\")
}

fn suspicious_space_divergence(space: DestinationSpaceSnapshot) -> bool {
    if space.volume_free_bytes <= space.available_bytes {
        return false;
    }

    let delta = space.volume_free_bytes - space.available_bytes;
    let delta_threshold = 8 * 1024 * 1024 * 1024; // 8 GiB
    let ratio_threshold_numerator = 4u64; // available < 80% of volume free
    let ratio_threshold_denominator = 5u64;

    delta >= delta_threshold
        && (space
            .available_bytes
            .saturating_mul(ratio_threshold_denominator)
            < space
                .volume_free_bytes
                .saturating_mul(ratio_threshold_numerator))
}

pub fn analyze_staging_preflight(
    config: &TransferConfig,
    snapshot: &PlanningSnapshot,
) -> Result<PreflightReport, CaravanError> {
    let probe = SystemDestinationProbe;
    analyze_staging_preflight_with_probe(config, snapshot, &probe)
}

pub fn analyze_staging_preflight_with_probe(
    config: &TransferConfig,
    snapshot: &PlanningSnapshot,
    probe: &dyn DestinationProbe,
) -> Result<PreflightReport, CaravanError> {
    if config.mode != Mode::Staging {
        return Ok(PreflightReport {
            warnings: Vec::new(),
            max_estimated_destination_path_len: 0,
            case_collision_count: 0,
        });
    }

    let mut warnings = Vec::new();

    let flags = probe.destination_flags(&config.dest)?;
    if flags.is_compressed {
        warnings.push(PreflightWarning {
            code: PreflightWarningCode::DestinationCompressed,
            message: format!(
                "destination '{}' is compressed; NTFS compression can reduce copy throughput for large media files",
                config.dest.display()
            ),
        });
    }
    if flags.is_reparse_point {
        warnings.push(PreflightWarning {
            code: PreflightWarningCode::DestinationReparsePoint,
            message: format!(
                "destination '{}' is a reparse point; verify target volume and available space before continuing",
                config.dest.display()
            ),
        });
    }

    let windows_destination = cfg!(windows) || destination_looks_windows_style(&config.dest);
    if windows_destination {
        if let Some(space) = probe.destination_space(&config.dest)? {
            if suspicious_space_divergence(space) {
                let delta = space
                    .volume_free_bytes
                    .saturating_sub(space.available_bytes);
                warnings.push(PreflightWarning {
                    code: PreflightWarningCode::SpaceAccountingDivergence,
                    message: format!(
                        "destination '{}' reports divergent free-space metrics: available_to_caller={} bytes, volume_free={} bytes, delta={} bytes; caravan capacity checks use available_to_caller",
                        config.dest.display(),
                        space.available_bytes,
                        space.volume_free_bytes,
                        delta
                    ),
                });
            }
        }
    }

    let mut case_key_to_original: HashMap<String, String> = HashMap::new();
    let mut case_collision_pairs: Vec<(String, String)> = Vec::new();
    let mut max_estimated_destination_path_len = 0usize;

    let dest_rendered = config.dest.to_string_lossy();
    let dest_len = dest_rendered.len();
    let needs_separator = !(dest_rendered.ends_with('\\') || dest_rendered.ends_with('/'));

    for batch in &snapshot.batches {
        for entry in &batch.files {
            let rel = entry.relative_path.to_string_lossy().to_string();
            let case_key = rel.to_lowercase();
            if let Some(existing) = case_key_to_original.get(&case_key) {
                if existing != &rel {
                    case_collision_pairs.push((existing.clone(), rel.clone()));
                }
            } else {
                case_key_to_original.insert(case_key, rel.clone());
            }

            let estimated_len = dest_len + usize::from(needs_separator) + rel.len();
            if estimated_len > max_estimated_destination_path_len {
                max_estimated_destination_path_len = estimated_len;
            }
        }
    }

    if !case_collision_pairs.is_empty() {
        let samples = case_collision_pairs
            .iter()
            .take(3)
            .map(|(left, right)| format!("'{}' vs '{}'", left, right))
            .collect::<Vec<_>>()
            .join(", ");
        warnings.push(PreflightWarning {
            code: PreflightWarningCode::CaseCollisionRisk,
            message: format!(
                "planned paths contain {} case-collision pair(s) on case-insensitive filesystems; samples: {}",
                case_collision_pairs.len(),
                samples
            ),
        });
    }

    if (cfg!(windows) || destination_looks_windows_style(&config.dest))
        && !destination_uses_extended_windows_prefix(&config.dest)
        && max_estimated_destination_path_len >= 240
    {
        warnings.push(PreflightWarning {
            code: PreflightWarningCode::PathLengthPressure,
            message: format!(
                "longest estimated destination path is {} chars; this is near legacy Windows path limits",
                max_estimated_destination_path_len
            ),
        });
    }

    Ok(PreflightReport {
        warnings,
        max_estimated_destination_path_len,
        case_collision_count: case_collision_pairs.len(),
    })
}
