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

pub trait FilesystemTypeProbe {
    fn filesystem_type(&self, path: &Path) -> Result<Option<String>, CaravanError>;
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

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemFilesystemTypeProbe;

impl FilesystemTypeProbe for SystemFilesystemTypeProbe {
    fn filesystem_type(&self, path: &Path) -> Result<Option<String>, CaravanError> {
        detect_filesystem_type(path)
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
        let last_error = std::io::Error::last_os_error();
        if last_error.kind() == std::io::ErrorKind::NotFound {
            return Ok(DestinationFlags {
                is_compressed: false,
                is_reparse_point: false,
            });
        }

        return Err(CaravanError::IoContext {
            context: format!(
                "failed to inspect destination attributes at {}",
                destination.display()
            ),
            source: last_error,
        });
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

#[cfg(target_os = "linux")]
fn detect_filesystem_type(path: &Path) -> Result<Option<String>, CaravanError> {
    let probe_path = resolve_probe_path(path);
    let content = std::fs::read_to_string("/proc/self/mountinfo").map_err(|source| {
        CaravanError::IoContext {
            context: "failed to read /proc/self/mountinfo".to_string(),
            source,
        }
    })?;

    let mut best_match: Option<(usize, String)> = None;
    let probe_rendered = probe_path.to_string_lossy().to_string();

    for line in content.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let left_fields: Vec<&str> = left.split_whitespace().collect();
        if left_fields.len() < 5 {
            continue;
        }

        let mount_point = decode_mountinfo_path(left_fields[4]);
        if !path_is_within_mount(&probe_rendered, &mount_point) {
            continue;
        }

        let mut right_fields = right.split_whitespace();
        let Some(fs_type) = right_fields.next() else {
            continue;
        };

        let mount_len = mount_point.len();
        let should_replace = best_match
            .as_ref()
            .map(|(best_len, _)| mount_len > *best_len)
            .unwrap_or(true);
        if should_replace {
            best_match = Some((mount_len, fs_type.to_string()));
        }
    }

    Ok(best_match.map(|(_, fs_type)| fs_type))
}

#[cfg(not(target_os = "linux"))]
fn detect_filesystem_type(_path: &Path) -> Result<Option<String>, CaravanError> {
    Ok(None)
}

#[cfg(target_os = "linux")]
fn resolve_probe_path(path: &Path) -> std::path::PathBuf {
    if path.exists() {
        return path.to_path_buf();
    }
    let mut candidate = path;
    while let Some(parent) = candidate.parent() {
        if parent.exists() {
            return parent.to_path_buf();
        }
        candidate = parent;
    }
    path.to_path_buf()
}

#[cfg(target_os = "linux")]
fn decode_mountinfo_path(value: &str) -> String {
    let mut decoded_path = String::new();
    let mut char_stream = value.chars().peekable();

    while let Some(current_char) = char_stream.next() {
        if current_char != '\\' {
            decoded_path.push(current_char);
            continue;
        }

        let first_digit = char_stream.next();
        let second_digit = char_stream.next();
        let third_digit = char_stream.next();

        if let (Some(first_digit), Some(second_digit), Some(third_digit)) =
            (first_digit, second_digit, third_digit)
        {
            if let Some(decoded_char) = decode_octal_escape(first_digit, second_digit, third_digit)
            {
                decoded_path.push(decoded_char);
                continue;
            }

            decoded_path.push('\\');
            decoded_path.push(first_digit);
            decoded_path.push(second_digit);
            decoded_path.push(third_digit);
            continue;
        }

        decoded_path.push('\\');
        if let Some(first_digit) = first_digit {
            decoded_path.push(first_digit);
        }
        if let Some(second_digit) = second_digit {
            decoded_path.push(second_digit);
        }
        if let Some(third_digit) = third_digit {
            decoded_path.push(third_digit);
        }
    }

    decoded_path
}

#[cfg(target_os = "linux")]
fn decode_octal_escape(first_digit: char, second_digit: char, third_digit: char) -> Option<char> {
    let first_value = first_digit.to_digit(8)?;
    let second_value = second_digit.to_digit(8)?;
    let third_value = third_digit.to_digit(8)?;
    let byte = ((first_value << 6) | (second_value << 3) | third_value) as u8;
    Some(byte as char)
}

#[cfg(target_os = "linux")]
fn path_is_within_mount(path: &str, mount_point: &str) -> bool {
    if mount_point == "/" {
        return path.starts_with('/');
    }
    path == mount_point
        || path
            .strip_prefix(mount_point)
            .map(|tail| tail.starts_with('/'))
            .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightWarningCode {
    DestinationCompressed,
    DestinationReparsePoint,
    PathLengthPressure,
    CaseCollisionRisk,
    SpaceAccountingDivergence,
    SourceFilesystemNotNtfsLike,
    DestinationFilesystemNotBtrfs,
    SourceFilesystemUnknown,
    DestinationFilesystemUnknown,
}

impl PreflightWarningCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            PreflightWarningCode::DestinationCompressed => "destination_compressed",
            PreflightWarningCode::DestinationReparsePoint => "destination_reparse_point",
            PreflightWarningCode::PathLengthPressure => "path_length_pressure",
            PreflightWarningCode::CaseCollisionRisk => "case_collision_risk",
            PreflightWarningCode::SpaceAccountingDivergence => "space_accounting_divergence",
            PreflightWarningCode::SourceFilesystemNotNtfsLike => "source_filesystem_not_ntfs_like",
            PreflightWarningCode::DestinationFilesystemNotBtrfs => {
                "destination_filesystem_not_btrfs"
            }
            PreflightWarningCode::SourceFilesystemUnknown => "source_filesystem_unknown",
            PreflightWarningCode::DestinationFilesystemUnknown => "destination_filesystem_unknown",
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

pub fn analyze_transfer_preflight(
    config: &TransferConfig,
    snapshot: &PlanningSnapshot,
) -> Result<PreflightReport, CaravanError> {
    let destination_probe = SystemDestinationProbe;
    let filesystem_probe = SystemFilesystemTypeProbe;
    analyze_transfer_preflight_with_probes(config, snapshot, &destination_probe, &filesystem_probe)
}

pub fn analyze_transfer_preflight_with_probes(
    config: &TransferConfig,
    snapshot: &PlanningSnapshot,
    destination_probe: &dyn DestinationProbe,
    filesystem_probe: &dyn FilesystemTypeProbe,
) -> Result<PreflightReport, CaravanError> {
    let staging = analyze_staging_preflight_with_probe(config, snapshot, destination_probe)?;
    let migrate = analyze_migrate_preflight_with_probe(config, filesystem_probe)?;
    Ok(merge_preflight_reports(staging, migrate))
}

pub fn enforce_transfer_preflight_policy(
    config: &TransferConfig,
    report: &PreflightReport,
) -> Result<(), CaravanError> {
    if config.mode != Mode::Migrate || config.allow_unsafe_filesystems {
        return Ok(());
    }

    let blocking_codes: Vec<&'static str> = report
        .warnings
        .iter()
        .filter(|warning| is_blocking_migrate_warning(warning.code))
        .map(|warning| warning.code.as_str())
        .collect();

    if blocking_codes.is_empty() {
        return Ok(());
    }

    let joined_codes = blocking_codes.join(", ");
    Err(CaravanError::InvalidArguments(format!(
        "migrate preflight blocked due to unsafe filesystem topology ({joined_codes}); verify source/destination mounts or re-run with --allow-unsafe-filesystems"
    )))
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

pub fn analyze_migrate_preflight(config: &TransferConfig) -> Result<PreflightReport, CaravanError> {
    let probe = SystemFilesystemTypeProbe;
    analyze_migrate_preflight_with_probe(config, &probe)
}

pub fn analyze_migrate_preflight_with_probe(
    config: &TransferConfig,
    probe: &dyn FilesystemTypeProbe,
) -> Result<PreflightReport, CaravanError> {
    if config.mode != Mode::Migrate {
        return Ok(PreflightReport {
            warnings: Vec::new(),
            max_estimated_destination_path_len: 0,
            case_collision_count: 0,
        });
    }

    let mut warnings = Vec::new();

    let source_fs = probe.filesystem_type(&config.source)?;
    match source_fs {
        Some(source_fs) if is_ntfs_like_filesystem(&source_fs) => {}
        Some(source_fs) => warnings.push(PreflightWarning {
            code: PreflightWarningCode::SourceFilesystemNotNtfsLike,
            message: format!(
                "source '{}' appears to be '{}' (expected NTFS-like: ntfs, ntfs3, fuseblk); verify staging mount before migration",
                config.source.display(),
                source_fs
            ),
        }),
        None => warnings.push(PreflightWarning {
            code: PreflightWarningCode::SourceFilesystemUnknown,
            message: format!(
                "could not determine source filesystem type for '{}'; expected NTFS-like staging source",
                config.source.display()
            ),
        }),
    }

    let destination_fs = probe.filesystem_type(&config.dest)?;
    match destination_fs {
        Some(destination_fs) if destination_fs.eq_ignore_ascii_case("btrfs") => {}
        Some(destination_fs) => warnings.push(PreflightWarning {
            code: PreflightWarningCode::DestinationFilesystemNotBtrfs,
            message: format!(
                "destination '{}' appears to be '{}' (expected btrfs) for migrate mode safety features",
                config.dest.display(),
                destination_fs
            ),
        }),
        None => warnings.push(PreflightWarning {
            code: PreflightWarningCode::DestinationFilesystemUnknown,
            message: format!(
                "could not determine destination filesystem type for '{}'; expected btrfs destination",
                config.dest.display()
            ),
        }),
    }

    Ok(PreflightReport {
        warnings,
        max_estimated_destination_path_len: 0,
        case_collision_count: 0,
    })
}

fn is_ntfs_like_filesystem(filesystem_type: &str) -> bool {
    let fs = filesystem_type.to_ascii_lowercase();
    fs == "ntfs" || fs == "ntfs3" || fs == "fuseblk" || fs == "fuse.ntfs-3g" || fs == "ntfs-3g"
}

fn merge_preflight_reports(left: PreflightReport, right: PreflightReport) -> PreflightReport {
    let mut warnings = left.warnings;
    warnings.extend(right.warnings);
    PreflightReport {
        warnings,
        max_estimated_destination_path_len: left
            .max_estimated_destination_path_len
            .max(right.max_estimated_destination_path_len),
        case_collision_count: left.case_collision_count + right.case_collision_count,
    }
}

fn is_blocking_migrate_warning(code: PreflightWarningCode) -> bool {
    matches!(
        code,
        PreflightWarningCode::SourceFilesystemNotNtfsLike
            | PreflightWarningCode::DestinationFilesystemNotBtrfs
            | PreflightWarningCode::SourceFilesystemUnknown
            | PreflightWarningCode::DestinationFilesystemUnknown
    )
}
