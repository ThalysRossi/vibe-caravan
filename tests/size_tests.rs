use caravan::cli::parse_cli_from;
use caravan::size::{SizeParseError, parse_size};

#[test]
fn parse_size_supports_binary_units() {
    assert_eq!(parse_size("1").expect("bytes"), 1);
    assert_eq!(parse_size("1B").expect("bytes"), 1);
    assert_eq!(parse_size("2KiB").expect("kib"), 2 * 1024);
    assert_eq!(parse_size("3MiB").expect("mib"), 3 * 1024 * 1024);
    assert_eq!(parse_size("4GiB").expect("gib"), 4 * 1024 * 1024 * 1024);
}

#[test]
fn parse_size_rejects_invalid_input() {
    assert_eq!(
        parse_size("").expect_err("empty should fail"),
        SizeParseError::Empty
    );
    assert_eq!(
        parse_size("abc").expect_err("missing digits should fail"),
        SizeParseError::MissingDigits
    );
    assert_eq!(
        parse_size("1MB").expect_err("unsupported unit should fail"),
        SizeParseError::UnsupportedUnit
    );
}

#[test]
fn parse_size_rejects_overflow() {
    assert_eq!(
        parse_size("18446744073709551615TiB").expect_err("overflow should fail"),
        SizeParseError::TooLarge
    );
}

#[test]
fn cli_batch_size_uses_caller_specific_wording() {
    let result = parse_cli_from([
        "caravan",
        "staging",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "abc",
    ]);
    let err = result.expect_err("cli parse should fail");
    assert!(
        err.to_string()
            .contains("batch-size must start with digits"),
        "error should preserve batch-size caller context: {err}"
    );
}
