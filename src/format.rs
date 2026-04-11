//! Utilities for formatting data for human-readable display

/// Format a byte count into a human-readable string using binary prefixes (KiB, MiB, GiB, TiB).
///
/// Uses binary prefixes where 1 KiB = 1024 bytes, 1 MiB = 1024² bytes, etc.
/// Shows 2 decimal places for sizes >= 1 KiB.
///
/// # Examples
/// ```
/// use caravan::format::format_bytes;
///
/// assert_eq!(format_bytes(0), "0 bytes");
/// assert_eq!(format_bytes(1023), "1023 bytes");
/// assert_eq!(format_bytes(1024), "1.00 KiB");
/// assert_eq!(format_bytes(1536), "1.50 KiB");  // 1.5 KiB
/// assert_eq!(format_bytes(1073741824), "1.00 GiB");
/// ```
pub fn format_bytes(bytes: u64) -> String {
    let unit = ByteUnit::for_bytes(bytes);
    
    match unit {
        ByteUnit::Bytes => {
            // Special handling for singular "byte" vs plural "bytes"
            if bytes == 1 {
                "1 byte".to_string()
            } else {
                format!("{} bytes", bytes)
            }
        }
        _ => {
            let value = bytes as f64 / unit.divisor();
            format!("{:.2} {}", value, unit.suffix())
        }
    }
}

/// Byte units for human-readable formatting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteUnit {
    Bytes,
    KiB,
    MiB,
    GiB,
    TiB,
}

impl ByteUnit {
    /// Determine the appropriate unit for a given byte count
    fn for_bytes(bytes: u64) -> ByteUnit {
        match bytes.checked_ilog(1024).unwrap_or(0).min(4) {
            0 => ByteUnit::Bytes,
            1 => ByteUnit::KiB,
            2 => ByteUnit::MiB,
            3 => ByteUnit::GiB,
            4 => ByteUnit::TiB,
            _ => unreachable!(), // min(4) ensures we never exceed 4
        }
    }
    
    /// Get the divisor for converting bytes to this unit
    fn divisor(&self) -> f64 {
        match self {
            ByteUnit::Bytes => 1.0,
            ByteUnit::KiB => 1024.0,
            ByteUnit::MiB => 1024.0 * 1024.0,
            ByteUnit::GiB => 1024.0 * 1024.0 * 1024.0,
            ByteUnit::TiB => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        }
    }
    
    /// Get the display suffix for this unit
    fn suffix(&self) -> &'static str {
        match self {
            ByteUnit::Bytes => "bytes",
            ByteUnit::KiB => "KiB",
            ByteUnit::MiB => "MiB",
            ByteUnit::GiB => "GiB",
            ByteUnit::TiB => "TiB",
        }
    }
}