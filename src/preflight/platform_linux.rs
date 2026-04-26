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

    let probe_rendered = probe_path.to_string_lossy().to_string();
    Ok(detect_filesystem_type_from_mountinfo(
        &probe_rendered,
        &content,
    ))
}

fn detect_filesystem_type_from_mountinfo(
    probe_path: &str,
    mountinfo_content: &str,
) -> Option<String> {
    let mut best_match: Option<(usize, String)> = None;

    for line in mountinfo_content.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let left_fields: Vec<&str> = left.split_whitespace().collect();
        if left_fields.len() < 5 {
            continue;
        }

        let mount_point = decode_mountinfo_path(left_fields[4]);
        if !path_is_within_mount(probe_path, &mount_point) {
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

    best_match.map(|(_, fs_type)| fs_type)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_mountinfo_path_decodes_octal_escapes() {
        let decoded = decode_mountinfo_path("/run/media/Game\\040SSD\\0403");
        assert_eq!(decoded, "/run/media/Game SSD 3");
    }

    #[test]
    fn decode_mountinfo_path_keeps_invalid_escape_literal() {
        let decoded = decode_mountinfo_path("/mnt/data\\89x");
        assert_eq!(decoded, "/mnt/data\\89x");
    }

    #[test]
    fn path_is_within_mount_handles_root_and_nested_mounts() {
        assert!(path_is_within_mount("/a/b", "/"));
        assert!(path_is_within_mount(
            "/run/media/disk/file.mkv",
            "/run/media/disk"
        ));
        assert!(!path_is_within_mount(
            "/run/media/disk2/file.mkv",
            "/run/media/disk"
        ));
    }

    #[test]
    fn resolve_probe_path_falls_back_to_existing_parent() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let existing_parent = tmp.path().join("a").join("b");
        std::fs::create_dir_all(&existing_parent).expect("create parent directories");

        let missing_leaf = existing_parent.join("missing").join("child");
        let resolved = resolve_probe_path(&missing_leaf);

        assert_eq!(resolved, existing_parent);
    }

    #[test]
    fn decode_octal_escape_handles_valid_and_invalid_digits() {
        assert_eq!(decode_octal_escape('0', '4', '0'), Some(' '));
        assert_eq!(decode_octal_escape('8', '4', '0'), None);
    }

    #[test]
    fn detect_filesystem_type_from_mountinfo_selects_longest_match() {
        let mountinfo = "\
25 21 8:1 / / rw,relatime - ext4 /dev/sda1 rw
44 25 8:2 / /run/media/Game\\040SSD\\0403 rw,relatime - ntfs3 /dev/sdb1 rw
45 44 8:3 / /run/media/Game\\040SSD\\0403/Media rw,relatime - xfs /dev/sdc1 rw
";
        let fs_type =
            detect_filesystem_type_from_mountinfo("/run/media/Game SSD 3/Media/Films", mountinfo);
        assert_eq!(fs_type.as_deref(), Some("xfs"));
    }

    #[test]
    fn detect_filesystem_type_from_mountinfo_ignores_malformed_lines() {
        let mountinfo = "\
this line has no separator
1 2 3 - ext4 /dev/sda1 rw
5 4 8:1 / /run/media rw,relatime - 
";
        let fs_type = detect_filesystem_type_from_mountinfo("/run/media/disk/file.mkv", mountinfo);
        assert_eq!(fs_type, None);
    }

    #[test]
    fn detect_filesystem_type_from_mountinfo_uses_root_mount_fallback() {
        let mountinfo = "10 2 8:1 / / rw,relatime - btrfs /dev/nvme0n1p2 rw";
        let fs_type = detect_filesystem_type_from_mountinfo("/home/thalys/file.txt", mountinfo);
        assert_eq!(fs_type.as_deref(), Some("btrfs"));
    }

    #[test]
    fn resolve_probe_path_returns_input_for_parentless_relative_path() {
        let input = Path::new("no-parent-segment");
        let resolved = resolve_probe_path(input);
        assert_eq!(resolved, input);
    }
}
