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
