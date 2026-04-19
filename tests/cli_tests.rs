use caravan::cli::parse_cli_from;
use caravan::config::{Config, ConflictPolicy, CopyStrategy, OutputFormat};

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
fn status_and_resume_output_formats_default_to_human_and_accept_json() {
    let status_default = parse_cli_from(["caravan", "status"]).expect("status parse should pass");
    let status_json = parse_cli_from(["caravan", "status", "--output", "json"])
        .expect("status parse with json output should pass");
    let resume_default = parse_cli_from(["caravan", "resume"]).expect("resume parse should pass");
    let resume_json = parse_cli_from(["caravan", "resume", "--inspect-failed", "--output", "json"])
        .expect("resume parse with json output should pass");

    match status_default {
        Config::Status { output, .. } => assert_eq!(output, OutputFormat::Human),
        _ => panic!("expected status config"),
    }
    match status_json {
        Config::Status { output, .. } => assert_eq!(output, OutputFormat::Json),
        _ => panic!("expected status config"),
    }
    match resume_default {
        Config::Resume { output, .. } => assert_eq!(output, OutputFormat::Human),
        _ => panic!("expected resume config"),
    }
    match resume_json {
        Config::Resume {
            output,
            inspect_failed,
            ..
        } => {
            assert_eq!(output, OutputFormat::Json);
            assert!(inspect_failed);
        }
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
        "--snapshot-every",
        "2",
    ])
    .expect("migrate parse should pass");

    match parsed {
        Config::Migrate(cfg) => {
            assert_eq!(cfg.batch_size_bytes, 2 * 1024 * 1024 * 1024);
            assert_eq!(cfg.snapshot_every, Some(2));
            assert_eq!(cfg.log_level, "debug");
        }
        _ => panic!("expected migrate config"),
    }
}

#[test]
fn resume_command_parses_default_state_path() {
    let result = parse_cli_from(["caravan", "resume"]).expect("resume command should parse");

    match result {
        Config::Resume { state, .. } => {
            assert_eq!(state.to_string_lossy(), ".caravan/state.json");
        }
        _ => panic!("expected resume config"),
    }
}

#[test]
fn resume_command_accepts_custom_state_path() {
    let result = parse_cli_from(["caravan", "resume", "--state", "/tmp/custom-state.json"])
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
    let result = parse_cli_from(["caravan", "--log-level", "trace", "resume"])
        .expect("resume with log level should parse");

    match result {
        Config::Resume { log_level, .. } => {
            assert_eq!(log_level, "trace");
        }
        _ => panic!("expected resume config"),
    }
}

#[test]
fn zero_copy_buffer_size_is_rejected() {
    let result = parse_cli_from([
        "caravan",
        "staging",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--copy-buffer-size",
        "0",
    ]);

    assert!(result.is_err());
}

#[test]
fn invalid_copy_buffer_size_preserves_option_specific_error_wording() {
    let result = parse_cli_from([
        "caravan",
        "staging",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--copy-buffer-size",
        "abc",
    ]);
    let err = result.expect_err("invalid copy-buffer-size should fail");
    assert!(
        err.to_string()
            .contains("copy-buffer-size: size must start with digits"),
        "expected option-specific message, got: {err}"
    );
}

#[test]
fn zero_buffered_copy_threshold_is_rejected() {
    let result = parse_cli_from([
        "caravan",
        "staging",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--buffered-copy-threshold",
        "0",
    ]);

    assert!(result.is_err());
}

#[test]
fn invalid_buffered_copy_threshold_preserves_option_specific_error_wording() {
    let result = parse_cli_from([
        "caravan",
        "staging",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--buffered-copy-threshold",
        "1MB",
    ]);
    let err = result.expect_err("invalid buffered-copy-threshold should fail");
    assert!(
        err.to_string().contains(
            "buffered-copy-threshold: unsupported size unit; use B, KiB, MiB, GiB, or TiB"
        ),
        "expected option-specific message, got: {err}"
    );
}

#[test]
fn staging_recover_failed_flag_is_parsed() {
    let result = parse_cli_from([
        "caravan",
        "staging",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--recover-failed",
    ])
    .expect("staging with recover-failed should parse");

    match result {
        Config::Staging(cfg) => assert!(cfg.recover_failed),
        _ => panic!("expected staging config"),
    }
}

#[test]
fn resume_recover_failed_flag_defaults_to_false_and_can_be_enabled() {
    let off = parse_cli_from(["caravan", "resume"]).expect("resume parse should pass");
    let on = parse_cli_from(["caravan", "resume", "--recover-failed"])
        .expect("resume parse with recover-failed should pass");

    match off {
        Config::Resume { recover_failed, .. } => assert!(!recover_failed),
        _ => panic!("expected resume config"),
    }

    match on {
        Config::Resume { recover_failed, .. } => assert!(recover_failed),
        _ => panic!("expected resume config"),
    }
}

#[test]
fn resume_inspect_failed_flag_defaults_to_false_and_can_be_enabled() {
    let off = parse_cli_from(["caravan", "resume"]).expect("resume parse should pass");
    let on = parse_cli_from(["caravan", "resume", "--inspect-failed"])
        .expect("resume parse with inspect-failed should pass");

    match off {
        Config::Resume { inspect_failed, .. } => assert!(!inspect_failed),
        _ => panic!("expected resume config"),
    }

    match on {
        Config::Resume { inspect_failed, .. } => assert!(inspect_failed),
        _ => panic!("expected resume config"),
    }
}

#[test]
fn copy_strategy_is_parsed_for_transfer_commands() {
    let parsed = parse_cli_from([
        "caravan",
        "staging",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--copy-strategy",
        "native",
    ])
    .expect("staging parse should accept copy-strategy");

    match parsed {
        Config::Staging(cfg) => assert_eq!(cfg.copy_strategy, CopyStrategy::Native),
        _ => panic!("expected staging config"),
    }
}

#[test]
fn invalid_copy_strategy_is_rejected() {
    let result = parse_cli_from([
        "caravan",
        "staging",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--copy-strategy",
        "invalid-strategy",
    ]);

    assert!(result.is_err());
}

#[test]
fn allow_unsafe_filesystems_flag_is_parsed_for_transfer_commands() {
    let default_parse = parse_cli_from([
        "caravan",
        "migrate",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
    ])
    .expect("migrate parse should pass");

    let enabled_parse = parse_cli_from([
        "caravan",
        "migrate",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--allow-unsafe-filesystems",
    ])
    .expect("migrate parse with allow-unsafe-filesystems should pass");

    match default_parse {
        Config::Migrate(cfg) => assert!(!cfg.allow_unsafe_filesystems),
        _ => panic!("expected migrate config"),
    }
    match enabled_parse {
        Config::Migrate(cfg) => assert!(cfg.allow_unsafe_filesystems),
        _ => panic!("expected migrate config"),
    }
}

#[test]
fn snapshot_dir_is_parsed_for_migrate_command() {
    let parsed = parse_cli_from([
        "caravan",
        "migrate",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--snapshot-every",
        "1",
        "--snapshot-dir",
        "/dst/snapshots",
    ])
    .expect("migrate parse with snapshot-dir should pass");

    match parsed {
        Config::Migrate(cfg) => {
            assert_eq!(
                cfg.snapshot_dir
                    .as_deref()
                    .map(|value| value.to_string_lossy().to_string()),
                Some("/dst/snapshots".to_string())
            );
        }
        _ => panic!("expected migrate config"),
    }
}

#[test]
fn snapshot_dir_requires_snapshot_every() {
    let result = parse_cli_from([
        "caravan",
        "migrate",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--snapshot-dir",
        "/dst/snapshots",
    ]);

    let err = result.expect_err("snapshot-dir without snapshot-every should fail");
    assert!(
        err.to_string()
            .contains("snapshot-dir requires snapshot-every")
    );
}

#[test]
fn verification_flag_is_rejected_for_transfer_commands() {
    let result = parse_cli_from([
        "caravan",
        "staging",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--verification",
        "strict",
    ]);

    assert!(result.is_err());
}

#[test]
fn conflict_policy_defaults_to_skip_batch_for_transfer_commands() {
    let staging = parse_cli_from([
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

    let migrate = parse_cli_from([
        "caravan",
        "migrate",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
    ])
    .expect("migrate parse should pass");

    match staging {
        Config::Staging(cfg) => assert_eq!(cfg.conflict_policy, ConflictPolicy::SkipBatch),
        _ => panic!("expected staging config"),
    }

    match migrate {
        Config::Migrate(cfg) => assert_eq!(cfg.conflict_policy, ConflictPolicy::SkipBatch),
        _ => panic!("expected migrate config"),
    }
}

#[test]
fn conflict_policy_is_parsed_for_transfer_commands() {
    let parsed = parse_cli_from([
        "caravan",
        "staging",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--conflict-policy",
        "skip-file",
    ])
    .expect("staging parse should accept conflict-policy");

    match parsed {
        Config::Staging(cfg) => assert_eq!(cfg.conflict_policy, ConflictPolicy::SkipFile),
        _ => panic!("expected staging config"),
    }
}

#[test]
fn invalid_conflict_policy_is_rejected() {
    let result = parse_cli_from([
        "caravan",
        "staging",
        "--source",
        "/src",
        "--dest",
        "/dst",
        "--batch-size",
        "1GiB",
        "--conflict-policy",
        "invalid-policy",
    ]);

    assert!(result.is_err());
}
