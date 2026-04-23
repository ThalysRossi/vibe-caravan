use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

#[test]
fn forced_pause_is_shown_for_cli_parse_errors() {
    let binary = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(binary)
        .args([
            "staging",
            "--source",
            "/src",
            "--dest",
            "/dst",
            "--batch-size",
            "1GiB",
            "unexpected",
        ])
        .env("CARAVAN_FORCE_PAUSE_ON_ERROR", "1")
        .write_stdin("\n")
        .output()
        .expect("run caravan with invalid argument");

    assert!(!output.status.success(), "invalid cli input must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("cli error:"), "stderr was: {stderr}");
    assert!(
        stderr.contains("Press Enter to exit..."),
        "stderr was: {stderr}"
    );
}

#[test]
fn forced_pause_is_shown_for_runtime_errors() {
    let tmp = TempDir::new().expect("create temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");

    fs::create_dir_all(&source_dir).expect("create source dir");
    fs::create_dir_all(&dest_dir).expect("create destination dir");
    fs::write(source_dir.join("file1.txt"), "content").expect("seed source file");

    // Force a source state-write failure before transfer by making source/.caravan a regular file.
    fs::write(source_dir.join(".caravan"), "not a directory")
        .expect("create blocking .caravan file");

    let binary = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(binary)
        .args([
            "staging",
            "--source",
            source_dir.to_str().expect("source path utf8"),
            "--dest",
            dest_dir.to_str().expect("destination path utf8"),
            "--batch-size",
            "1MiB",
        ])
        .env("CARAVAN_FORCE_PAUSE_ON_ERROR", "1")
        .write_stdin("\n")
        .current_dir(tmp.path())
        .output()
        .expect("run caravan with runtime error");

    assert!(!output.status.success(), "runtime error must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("cannot write state to source directory"),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains("Press Enter to exit..."),
        "stderr was: {stderr}"
    );
}

#[test]
fn pause_prompt_is_not_shown_by_default_in_non_tty_context() {
    let binary = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(binary)
        .args([
            "staging",
            "--source",
            "/src",
            "--dest",
            "/dst",
            "--batch-size",
            "1GiB",
            "unexpected",
        ])
        .output()
        .expect("run caravan with invalid argument");

    assert!(!output.status.success(), "invalid cli input must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("cli error:"), "stderr was: {stderr}");
    assert!(
        !stderr.contains("Press Enter to exit..."),
        "stderr was: {stderr}"
    );
}

#[test]
fn interactive_flag_enables_pause_prompt_in_non_tty_context() {
    let binary = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(binary)
        .args([
            "staging",
            "--source",
            "/src",
            "--dest",
            "/dst",
            "--batch-size",
            "1GiB",
            "--interactive",
            "unexpected",
        ])
        .output()
        .expect("run caravan with interactive invalid argument");

    assert!(!output.status.success(), "invalid cli input must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("cli error:"), "stderr was: {stderr}");
    assert!(
        stderr.contains("Press Enter to exit..."),
        "stderr was: {stderr}"
    );
}
