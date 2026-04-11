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
    if bytes < 1024 {
        if bytes == 1 {
            "1 byte".to_string()
        } else {
            format!("{} bytes", bytes)
        }
    } else if bytes < 1024 * 1024 {
        // KiB range
        let kib = bytes as f64 / 1024.0;
        format!("{:.2} KiB", kib)
    } else if bytes < 1024 * 1024 * 1024 {
        // MiB range
        let mib = bytes as f64 / (1024.0 * 1024.0);
        format!("{:.2} MiB", mib)
    } else if bytes < 1024 * 1024 * 1024 * 1024 {
        // GiB range
        let gib = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        format!("{:.2} GiB", gib)
    } else {
        // TiB range (and beyond)
        let tib = bytes as f64 / (1024.0 * 1024.0 * 1024.0 * 1024.0);
        format!("{:.2} TiB", tib)
    }
}