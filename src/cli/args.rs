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
        Some(s) => parse_size_as_usize(s)
            .map_err(|e| CaravanError::InvalidArguments(format!("copy-buffer-size: {e}")))?,
        None => TransferConfig::default_copy_buffer_size(),
    };

    let buffered_copy_threshold = match buffered_copy_threshold {
        Some(s) => parse_size_with_label(s, "size")
            .map_err(|e| CaravanError::InvalidArguments(format!("buffered-copy-threshold: {e}")))?,
        None => TransferConfig::default_buffered_copy_threshold(),
    };

    validate_copy_option_values(copy_buffer_size, buffered_copy_threshold)?;

    Ok((copy_buffer_size, buffered_copy_threshold))
}

pub(crate) fn validate_copy_option_values(
    copy_buffer_size: usize,
    buffered_copy_threshold: u64,
) -> Result<(), CaravanError> {
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

    Ok(())
}

pub(super) fn parse_batch_size(input: &str) -> Result<u64, String> {
    parse_size_with_label(input, "batch-size")
}

fn parse_size_as_usize(input: &str) -> Result<usize, String> {
    let parsed = parse_size_with_label(input, "size")?;
    usize::try_from(parsed).map_err(|_| "size exceeds maximum allowed value".to_string())
}

fn parse_size_with_label(input: &str, label: &str) -> Result<u64, String> {
    crate::size::parse_size(input).map_err(|err| format_size_error(label, err))
}
