use std::path::{Path, PathBuf};

use crate::error::CaravanError;

use super::DestinationFlags;

pub(super) fn query_destination_flags(
    _destination: &Path,
) -> Result<DestinationFlags, CaravanError> {
    Ok(DestinationFlags {
        is_compressed: false,
        is_reparse_point: false,
    })
}

pub(super) fn detect_filesystem_type(path: &Path) -> Result<Option<String>, CaravanError> {
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

fn resolve_probe_path(path: &Path) -> PathBuf {
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

fn decode_octal_escape(first_digit: char, second_digit: char, third_digit: char) -> Option<char> {
    let first_value = first_digit.to_digit(8)?;
    let second_value = second_digit.to_digit(8)?;
    let third_value = third_digit.to_digit(8)?;
    let byte = ((first_value << 6) | (second_value << 3) | third_value) as u8;
    Some(byte as char)
}

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
