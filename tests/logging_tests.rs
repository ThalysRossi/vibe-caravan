use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

#[test]
fn invalid_log_level_is_rejected_at_runtime() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    fs::create_dir_all(&source_dir).expect("create source");
    fs::create_dir_all(&dest_dir).expect("create dest");
    fs::write(source_dir.join("file1.txt"), "content").expect("create source file");

    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    let output = Command::new(binary_path)
        .args([
            "--log-level",
            "verbose",
            "staging",
            "--source",
            source_dir.to_str().expect("utf8 source"),
            "--dest",
            dest_dir.to_str().expect("utf8 dest"),
            "--batch-size",
            "1MiB",
        ])
        .current_dir(tmp.path())
        .output()
        .expect("execute caravan");

    assert!(!output.status.success(), "invalid log level should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unsupported log-level"),
        "stderr should explain invalid log level: {stderr}"
    );
}
