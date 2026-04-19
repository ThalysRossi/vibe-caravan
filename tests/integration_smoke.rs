use assert_cmd::Command;

#[test]
fn integration_smoke_help_command_reports_cli_contract() {
    let binary = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(binary)
        .arg("--help")
        .output()
        .expect("run --help");

    assert!(
        !output.status.success(),
        "current CLI contract returns a non-zero exit code for --help"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Safe staged data migration tool"));
    assert!(stderr.contains("Usage: caravan"));
    assert!(stderr.contains("staging"));
    assert!(stderr.contains("resume"));
}

#[test]
fn staging_help_reports_conflict_policy_default() {
    let binary = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(binary)
        .args(["staging", "--help"])
        .output()
        .expect("run staging --help");

    assert!(
        !output.status.success(),
        "current CLI contract returns a non-zero exit code for --help"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--conflict-policy <CONFLICT_POLICY>"));
    assert!(stderr.contains("[default: skip-file]"));
    assert!(stderr.contains("[possible values: skip-batch, skip-file]"));
}
