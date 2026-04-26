#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeParseError {
    Empty,
    MissingDigits,
    InvalidNumericPart,
    UnsupportedUnit,
    TooLarge,
}

pub fn parse_size(input: &str) -> Result<u64, SizeParseError> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(SizeParseError::Empty);
    }

    let split_idx = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
    let (number, unit_raw) = raw.split_at(split_idx);
    if number.is_empty() {
        return Err(SizeParseError::MissingDigits);
    }

    let base = number
        .parse::<u64>()
        .map_err(|_| SizeParseError::InvalidNumericPart)?;
    let unit = unit_raw.trim().to_ascii_lowercase();

    let multiplier = match unit.as_str() {
        "" | "b" => 1_u64,
        "kib" => 1024_u64,
        "mib" => 1024_u64.pow(2),
        "gib" => 1024_u64.pow(3),
        "tib" => 1024_u64.pow(4),
        _ => return Err(SizeParseError::UnsupportedUnit),
    };

    base.checked_mul(multiplier).ok_or(SizeParseError::TooLarge)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_size_supports_bytes_and_binary_units() {
        assert_eq!(parse_size("42").expect("bytes"), 42);
        assert_eq!(parse_size("42b").expect("bytes"), 42);
        assert_eq!(parse_size("2KiB").expect("kib"), 2 * 1024);
        assert_eq!(parse_size("3MiB").expect("mib"), 3 * 1024 * 1024);
        assert_eq!(
            parse_size("1 GiB").expect("gib"),
            1024_u64 * 1024_u64 * 1024_u64
        );
        assert_eq!(parse_size("1TiB").expect("tib"), 1024_u64.pow(4));
    }

    #[test]
    fn parse_size_reports_empty_and_missing_digits() {
        assert_eq!(parse_size("   ").expect_err("empty"), SizeParseError::Empty);
        assert_eq!(
            parse_size("KiB").expect_err("missing digits"),
            SizeParseError::MissingDigits
        );
    }

    #[test]
    fn parse_size_reports_invalid_numeric_part_and_unit() {
        assert_eq!(
            parse_size("18446744073709551616").expect_err("u64 overflow must be invalid numeric"),
            SizeParseError::InvalidNumericPart
        );
        assert_eq!(
            parse_size("10MB").expect_err("unsupported unit"),
            SizeParseError::UnsupportedUnit
        );
    }

    #[test]
    fn parse_size_reports_too_large_multiplication() {
        let too_large = format!("{}TiB", u64::MAX);
        assert_eq!(
            parse_size(&too_large).expect_err("multiplication overflow"),
            SizeParseError::TooLarge
        );
    }
}
