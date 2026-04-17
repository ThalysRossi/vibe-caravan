use crate::config::TransferConfig;
use crate::error::CaravanError;
use crate::size::SizeParseError;

fn format_size_error(label: &str, err: SizeParseError) -> String {
    match err {
        SizeParseError::Empty => format!("{label} cannot be empty"),
        SizeParseError::MissingDigits => format!("{label} must start with digits"),
        SizeParseError::InvalidNumericPart => format!("{label} numeric part is invalid"),
        SizeParseError::UnsupportedUnit => {
            format!("unsupported {label} unit; use B, KiB, MiB, GiB, or TiB")
        }
        SizeParseError::TooLarge => format!("{label} is too large"),
    }
}

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
    crate::size::parse_size(input).map_err(|err| format_size_error("batch-size", err))
}

fn parse_size_usize(input: &str) -> Result<usize, String> {
    let result = parse_size_u64(input)?;

    if result > usize::MAX as u64 {
        return Err("size exceeds maximum allowed value".to_string());
    }

    Ok(result as usize)
}

fn parse_size_u64(input: &str) -> Result<u64, String> {
    crate::size::parse_size(input).map_err(|err| format_size_error("size", err))
}
