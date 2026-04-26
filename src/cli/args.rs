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

pub(super) fn parse_batch_size(input: &str) -> Result<u64, String> {
    parse_size_with_label(input, "batch-size")
}

fn parse_size_with_label(input: &str, label: &str) -> Result<u64, String> {
    crate::size::parse_size(input).map_err(|err| format_size_error(label, err))
}
