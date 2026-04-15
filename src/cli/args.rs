use crate::config::TransferConfig;
use crate::error::CaravanError;

pub(super) fn parse_copy_options(
    copy_buffer_size: Option<&str>,
    buffered_copy_threshold: Option<&str>,
) -> Result<(usize, u64), CaravanError> {
    let copy_buffer_size = match copy_buffer_size {
        Some(s) => parse_size_usize(s)
            .map_err(|e| CaravanError::InvalidArguments(format!("copy-buffer-size: {e}")))?,
        None => TransferConfig::default_copy_buffer_size(),
    };

    let buffered_copy_threshold = match buffered_copy_threshold {
        Some(s) => parse_size_u64(s)
            .map_err(|e| CaravanError::InvalidArguments(format!("buffered-copy-threshold: {e}")))?,
        None => TransferConfig::default_buffered_copy_threshold(),
    };

    if copy_buffer_size == 0 {
        return Err(CaravanError::InvalidArguments(
            "copy-buffer-size: size must be greater than zero".to_string(),
        ));
    }

    if buffered_copy_threshold == 0 {
        return Err(CaravanError::InvalidArguments(
            "buffered-copy-threshold: size must be greater than zero".to_string(),
        ));
    }

    Ok((copy_buffer_size, buffered_copy_threshold))
}

pub(super) fn parse_batch_size(input: &str) -> Result<u64, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("batch-size cannot be empty".to_string());
    }

    let split_idx = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
    let (number, unit_raw) = raw.split_at(split_idx);
    if number.is_empty() {
        return Err("batch-size must start with digits".to_string());
    }

    let base = number
        .parse::<u64>()
        .map_err(|_| "batch-size numeric part is invalid".to_string())?;
    let unit = unit_raw.trim().to_ascii_lowercase();

    let multiplier = match unit.as_str() {
        "" | "b" => 1_u64,
        "kib" => 1024_u64,
        "mib" => 1024_u64.pow(2),
        "gib" => 1024_u64.pow(3),
        "tib" => 1024_u64.pow(4),
        _ => {
            return Err("unsupported batch-size unit; use B, KiB, MiB, GiB, or TiB".to_string());
        }
    };

    base.checked_mul(multiplier)
        .ok_or_else(|| "batch-size is too large".to_string())
}

fn parse_size_usize(input: &str) -> Result<usize, String> {
    let result = parse_size_u64(input)?;

    if result > usize::MAX as u64 {
        return Err("size exceeds maximum allowed value".to_string());
    }

    Ok(result as usize)
}

fn parse_size_u64(input: &str) -> Result<u64, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("size cannot be empty".to_string());
    }

    let split_idx = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
    let (number, unit_raw) = raw.split_at(split_idx);
    if number.is_empty() {
        return Err("size must start with digits".to_string());
    }

    let base = number
        .parse::<u64>()
        .map_err(|_| "size numeric part is invalid".to_string())?;
    let unit = unit_raw.trim().to_ascii_lowercase();

    let multiplier = match unit.as_str() {
        "" | "b" => 1_u64,
        "kib" => 1024_u64,
        "mib" => 1024_u64.pow(2),
        "gib" => 1024_u64.pow(3),
        "tib" => 1024_u64.pow(4),
        _ => return Err("unsupported size unit; use B, KiB, MiB, GiB, or TiB".to_string()),
    };

    base.checked_mul(multiplier)
        .ok_or_else(|| "size is too large".to_string())
}
