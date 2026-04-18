use std::fs;
use std::process::Command;

#[test]
fn architecture_document_defines_cfg_policy() {
    let content = fs::read_to_string("ARCHITECTURE.md").expect("failed to read ARCHITECTURE.md");

    assert!(
        content.contains("Conditional Compilation Policy"),
        "ARCHITECTURE.md must define a Conditional Compilation Policy section"
    );
    assert!(
        content.contains("target_os = \"windows\"") && content.contains("target_os = \"linux\""),
        "ARCHITECTURE.md must explicitly document allowed target_os predicates"
    );
    assert!(
        content.contains("cfg!(...) runtime checks must be centralized in src/platform.rs"),
        "ARCHITECTURE.md must document runtime cfg centralization policy"
    );
}

#[test]
fn cfg_policy_check_script_exists_and_passes() {
    let output = Command::new("bash")
        .arg("scripts/check_cfg_policy.sh")
        .output()
        .expect("failed to execute scripts/check_cfg_policy.sh");

    assert!(
        output.status.success(),
        "cfg policy check script failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
