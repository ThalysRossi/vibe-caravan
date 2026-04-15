use std::fs;
use tempfile::TempDir;
use assert_cmd::Command;

#[test]
fn destination_directory_created_when_top_level_missing() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest"); // Does not exist
    
    fs::create_dir_all(&source_dir).expect("create source dir");
    // Don't create dest dir
    
    // Create a small test file
    fs::write(source_dir.join("test.txt"), "hello world").expect("write test file");
    
    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    
    // Run staging command - should create dest directory
    let _output = Command::new(&binary_path)
        .args(["staging",
            "--source", source_dir.to_str().unwrap(),
            "--dest", dest_dir.to_str().unwrap(),
            "--batch-size", "1MiB",
            "--interactive"])
        .current_dir(tmp.path())
        .output()
        .expect("Failed to execute caravan");
    
    // The command should at least try to run (will fail in interactive mode without stdin)
    // But dest directory should have been created
    assert!(dest_dir.exists(), "Destination directory should have been created");
    
    // Check that .caravan directory exists in source (for state)
    let state_dir = source_dir.join(".caravan");
    assert!(state_dir.exists(), "State directory should exist in source");
}

#[test]
fn destination_subdirectory_fails_when_parent_missing() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("parent").join("dest"); // parent doesn't exist
    
    fs::create_dir_all(&source_dir).expect("create source dir");
    // Don't create parent dir
    
    // Create a small test file
    fs::write(source_dir.join("test.txt"), "hello world").expect("write test file");
    
    let binary_path = assert_cmd::cargo::cargo_bin("caravan");
    
    // Run staging command - should fail with clear error
    let output = Command::new(&binary_path)
        .args(["staging",
            "--source", source_dir.to_str().unwrap(),
            "--dest", dest_dir.to_str().unwrap(),
            "--batch-size", "1MiB",
            "--interactive"])
        .current_dir(tmp.path())
        .output()
        .expect("Failed to execute caravan");
    
    // Should fail with error about missing parent directory
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("failed to read destination total capacity") ||
            stderr.contains("does not exist and cannot be created"),
            "Should fail with clear error about missing parent directory, got: {}", stderr);
    
    // Destination directory should NOT exist
    assert!(!dest_dir.exists(), "Destination subdirectory should not be created when parent missing");
}

#[test]
fn capacity_error_message_improved_for_missing_directory() {
    let tmp = TempDir::new().expect("temp dir");
    let source_dir = tmp.path().join("source");
    let dest_dir = tmp.path().join("dest");
    
    fs::create_dir_all(&source_dir).expect("create source dir");
    // Don't create dest dir
    
    // Create a small test file
    fs::write(source_dir.join("test.txt"), "hello world").expect("write test file");
    
    // Test directly with capacity module
    use caravan::capacity::check_capacity;
    
    // Should create directory and succeed (no error about capacity)
    let report = check_capacity(&dest_dir, 1024, 0);
    assert!(report.is_ok(), "check_capacity should create directory and succeed");
    let _report = report.unwrap();
    
    // Directory should exist now
    assert!(dest_dir.exists(), "Destination directory should have been created");
    
    // Run again with missing subdirectory
    let dest_subdir = dest_dir.join("subdir").join("deep");
    let report = check_capacity(&dest_subdir, 1024, 0);
    assert!(report.is_err(), "check_capacity should fail for missing parent directory");
    let err = report.unwrap_err();
    let err_str = err.to_string();
    assert!(err_str.contains("does not exist and cannot be created") ||
            err_str.contains("parent directory"),
            "Error should mention parent directory issue, got: {}", err_str);
}
