//! Tests for byte formatting utilities

use caravan::format::format_bytes;

#[test]
fn format_bytes_zero() {
    assert_eq!(format_bytes(0), "0 bytes");
}

#[test]
fn format_bytes_single_byte() {
    assert_eq!(format_bytes(1), "1 byte");
}

#[test]
fn format_bytes_few_bytes() {
    assert_eq!(format_bytes(500), "500 bytes");
}

#[test]
fn format_bytes_less_than_one_kib() {
    assert_eq!(format_bytes(1023), "1023 bytes");
}

#[test]
fn format_bytes_exactly_one_kib() {
    assert_eq!(format_bytes(1024), "1.00 KiB");
}

#[test]
fn format_bytes_slightly_more_than_one_kib() {
    assert_eq!(format_bytes(1025), "1.00 KiB");
}

#[test]
fn format_bytes_kib_with_decimal() {
    // 1.5 KiB = 1.5 * 1024 = 1536
    assert_eq!(format_bytes(1536), "1.50 KiB");
}

#[test]
fn format_bytes_just_below_one_mib() {
    // 1 MiB = 1024 KiB = 1048576 bytes
    assert_eq!(format_bytes(1048575), "1024.00 KiB");
}

#[test]
fn format_bytes_exactly_one_mib() {
    assert_eq!(format_bytes(1048576), "1.00 MiB");
}

#[test]
fn format_bytes_mib_with_decimal() {
    // 2.75 MiB = 2.75 * 1048576 = 2883584
    assert_eq!(format_bytes(2883584), "2.75 MiB");
}

#[test]
fn format_bytes_just_below_one_gib() {
    // 1 GiB = 1024 MiB = 1073741824 bytes
    assert_eq!(format_bytes(1073741823), "1024.00 MiB");
}

#[test]
fn format_bytes_exactly_one_gib() {
    assert_eq!(format_bytes(1073741824), "1.00 GiB");
}

#[test]
fn format_bytes_gib_with_decimal() {
    // 1.5 GiB = 1.5 * 1073741824 = 1610612736
    assert_eq!(format_bytes(1610612736), "1.50 GiB");
}

#[test]
fn format_bytes_large_gib() {
    // 100.25 GiB = 100.25 * 1073741824 = 107639910400
    assert_eq!(format_bytes(107639910400), "100.25 GiB");
}

#[test]
fn format_bytes_just_below_one_tib() {
    // 1 TiB = 1024 GiB = 1099511627776 bytes
    assert_eq!(format_bytes(1099511627775), "1024.00 GiB");
}

#[test]
fn format_bytes_exactly_one_tib() {
    assert_eq!(format_bytes(1099511627776), "1.00 TiB");
}

#[test]
fn format_bytes_tib_with_decimal() {
    // 2.33 TiB = 2.33 * 1099511627776 = 2561857411072
    assert_eq!(format_bytes(2561857411072), "2.33 TiB");
}

#[test]
fn format_bytes_very_large() {
    // 1024 TiB = 1 PiB (but we only go up to TiB for now)
    assert_eq!(format_bytes(1125899906842624), "1024.00 TiB");
}

#[test]
fn format_bytes_rounding_down() {
    // 1.234 KiB = 1264 bytes (1264 / 1024 = 1.234375)
    // Should round to 2 decimal places: 1.23 KiB
    assert_eq!(format_bytes(1264), "1.23 KiB");
}

#[test]
fn format_bytes_rounding_up() {
    // 1.235 KiB = 1265 bytes (1265 / 1024 = 1.2353515625)
    // Should round to 2 decimal places: 1.24 KiB
    assert_eq!(format_bytes(1265), "1.24 KiB");
}

#[test]
fn format_bytes_exact_half_rounding() {
    // Testing bankers rounding (round half to even)
    // 1.225 KiB = 1254 bytes (1254 / 1024 = 1.224609375)
    // Actually not exactly half, but let's test a real half case:
    // 1.255 KiB would be 1285 bytes (1285 / 1024 = 1.2548828125)
    // We'll test with 1.005 which is 1029 bytes (1029 / 1024 = 1.0048828125)
    // Should round to 1.00 KiB
    assert_eq!(format_bytes(1029), "1.00 KiB");
}

#[test]
fn format_bytes_batch_size_example() {
    // Example from the issue: 100 batches of 1GB each = 100 GiB
    // 100 GiB = 100 * 1073741824 = 107374182400
    assert_eq!(format_bytes(107374182400), "100.00 GiB");
}
