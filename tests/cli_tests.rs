use caravan::cli::parse_cli_from;
use caravan::config::{Config, VerificationMode};

#[test]
fn missing_required_arguments_are_rejected() {
    let result = parse_cli_from(["caravan", "staging"]);
    assert!(result.is_err());
}

#[test]
fn invalid_batch_sizes_are_rejected() {
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
    assert!(result.is_err());
}

#[test]
fn invalid_snapshot_settings_in_staging_are_rejected() {
    let result = parse_cli_from([
        "caravan",
        "staging",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--snapshot-every",
        "2",
    ]);
    assert!(result.is_err());
}

#[test]
fn interactive_mode_is_enabled_and_disabled_correctly() {
    let off = parse_cli_from([
        "caravan",
        "staging",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
    ])
    .expect("staging parse should pass");

    let on = parse_cli_from([
        "caravan",
        "staging",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--interactive",
    ])
    .expect("staging parse should pass");

    match off {
        Config::Staging(cfg) => assert!(!cfg.interactive),
        _ => panic!("expected staging config"),
    }
    match on {
        Config::Staging(cfg) => assert!(cfg.interactive),
        _ => panic!("expected staging config"),
    }
}

#[test]
fn state_file_path_defaults_are_applied_correctly() {
    let status = parse_cli_from(["caravan", "status"]).expect("status parse should pass");
    let resume = parse_cli_from(["caravan", "resume"]).expect("resume parse should pass");

    match status {
        Config::Status { state, .. } => assert_eq!(state.to_string_lossy(), ".caravan/state.json"),
        _ => panic!("expected status config"),
    }
    match resume {
        Config::Resume { state, .. } => assert_eq!(state.to_string_lossy(), ".caravan/state.json"),
        _ => panic!("expected resume config"),
    }
}

#[test]
fn source_and_destination_ordering_is_validated_by_mode() {
    let result = parse_cli_from([
        "caravan",
        "migrate",
        "--source",
        "/same",
        "--dest",
        "/same",
        "--batch-size",
        "1GiB",
    ]);
    assert!(result.is_err());
}

#[test]
fn mutually_exclusive_or_invalid_values_are_rejected() {
    let result = parse_cli_from([
        "caravan",
        "migrate",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--snapshot-every",
        "0",
    ]);
    assert!(result.is_err());
}

#[test]
fn parsed_configuration_is_typed_and_deterministic() {
    let parsed = parse_cli_from([
        "caravan",
        "--log-level",
        "debug",
        "migrate",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "2GiB",
        "--verification",
        "strict",
        "--snapshot-every",
        "2",
    ])
    .expect("migrate parse should pass");

    match parsed {
        Config::Migrate(cfg) => {
            assert_eq!(cfg.batch_size_bytes, 2 * 1024 * 1024 * 1024);
            assert_eq!(cfg.verification, VerificationMode::Strict);
            assert_eq!(cfg.snapshot_every, Some(2));
            assert_eq!(cfg.log_level, "debug");
        }
        _ => panic!("expected migrate config"),
    }
}

#[test]
fn resume_command_parses_default_state_path() {
    let result = parse_cli_from(["caravan", "resume"])
        .expect("resume command should parse");
    
    match result {
        Config::Resume { state, .. } => {
            assert_eq!(state.to_string_lossy(), ".caravan/state.json");
        }
        _ => panic!("expected resume config"),
    }
}

#[test]
fn resume_command_accepts_custom_state_path() {
    let result = parse_cli_from([
        "caravan",
        "resume",
        "--state",
        "/tmp/custom-state.json"
    ])
    .expect("resume with custom state path should parse");
    
    match result {
        Config::Resume { state, .. } => {
            assert_eq!(state.to_string_lossy(), "/tmp/custom-state.json");
        }
        _ => panic!("expected resume config"),
    }
}

#[test]
fn resume_config_includes_correct_log_level() {
    let result = parse_cli_from([
        "caravan",
        "--log-level",
        "trace",
        "resume"
    ])
    .expect("resume with log level should parse");
    
    match result {
        Config::Resume { log_level, .. } => {
            assert_eq!(log_level, "trace");
        }
        _ => panic!("expected resume config"),
    }
}
