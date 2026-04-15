use caravan::scan::{active_scan_backend, scan_source, scan_source_with_backend, ScanBackend};
use std::fs;
use tempfile::TempDir;

#[test]
fn scan_excludes_caravan_directory_at_root() {
    let tmp = TempDir::new().expect("temp dir");

    // Create regular files
    fs::write(tmp.path().join("file1.txt"), "content1").expect("create file1");
    fs::write(tmp.path().join("file2.txt"), "content2").expect("create file2");

    // Create .caravan directory with files
    let caravan_dir = tmp.path().join(".caravan");
    fs::create_dir_all(&caravan_dir).expect("create .caravan dir");
    fs::write(caravan_dir.join("state.json"), "{}").expect("create state file");
    fs::write(caravan_dir.join("migration_abc.json"), "{}").expect("create migration file");

    // Create subdirectory in .caravan
    let caravan_sub = caravan_dir.join("subdir");
    fs::create_dir_all(&caravan_sub).expect("create .caravan/subdir");
    fs::write(caravan_sub.join("config.txt"), "config").expect("create config file");

    // Scan source
    let scanned = scan_source(tmp.path()).expect("scan should succeed");

    // Verify .caravan files are NOT in scan results
    let scanned_paths: Vec<String> = scanned
        .iter()
        .map(|e| e.relative_path.to_string_lossy().to_string())
        .collect();

    // Should only have the regular files
    assert_eq!(scanned.len(), 2, "Should only scan 2 regular files");
    assert!(scanned_paths.contains(&"file1.txt".to_string()));
    assert!(scanned_paths.contains(&"file2.txt".to_string()));

    // Should NOT contain any .caravan paths
    assert!(
        !scanned_paths.iter().any(|p| p.contains(".caravan")),
        "Scan should not include .caravan files: {:?}",
        scanned_paths
    );
}

#[test]
fn scan_excludes_caravan_directory_in_subdirectory() {
    let tmp = TempDir::new().expect("temp dir");

    // Create directory structure
    let subdir = tmp.path().join("docs");
    fs::create_dir_all(&subdir).expect("create docs dir");

    // Create regular files
    fs::write(subdir.join("doc1.txt"), "doc1").expect("create doc1");
    fs::write(subdir.join("doc2.txt"), "doc2").expect("create doc2");

    // Create .caravan directory inside docs
    let caravan_dir = subdir.join(".caravan");
    fs::create_dir_all(&caravan_dir).expect("create docs/.caravan dir");
    fs::write(caravan_dir.join("cache.json"), "cache").expect("create cache file");

    // Scan source
    let scanned = scan_source(tmp.path()).expect("scan should succeed");

    // Verify .caravan files are NOT in scan results
    let scanned_paths: Vec<String> = scanned
        .iter()
        .map(|e| e.relative_path.to_string_lossy().to_string())
        .collect();

    // Should only have the regular files
    assert_eq!(scanned.len(), 2, "Should only scan 2 regular files");
    assert!(scanned_paths.contains(&"docs/doc1.txt".to_string()));
    assert!(scanned_paths.contains(&"docs/doc2.txt".to_string()));

    // Should NOT contain any .caravan paths
    assert!(
        !scanned_paths.iter().any(|p| p.contains(".caravan")),
        "Scan should not include .caravan files: {:?}",
        scanned_paths
    );
}

#[test]
fn scan_includes_other_hidden_directories() {
    let tmp = TempDir::new().expect("temp dir");

    // Create .git directory (should be included)
    let git_dir = tmp.path().join(".git");
    fs::create_dir_all(&git_dir).expect("create .git dir");
    fs::write(git_dir.join("config"), "[core]").expect("create git config");

    // Create .hidden file (should be included)
    fs::write(tmp.path().join(".hidden"), "secret").expect("create .hidden file");

    // Create regular file
    fs::write(tmp.path().join("visible.txt"), "visible").expect("create visible file");

    // Scan source
    let scanned = scan_source(tmp.path()).expect("scan should succeed");

    // Verify .git and .hidden ARE included (only .caravan excluded)
    let scanned_paths: Vec<String> = scanned
        .iter()
        .map(|e| e.relative_path.to_string_lossy().to_string())
        .collect();

    // Should have all 3 files
    assert_eq!(
        scanned.len(),
        3,
        "Should scan 3 files (.git/config, .hidden, visible.txt)"
    );
    assert!(scanned_paths.contains(&".git/config".to_string()));
    assert!(scanned_paths.contains(&".hidden".to_string()));
    assert!(scanned_paths.contains(&"visible.txt".to_string()));
}

#[test]
fn active_scan_backend_is_platform_aware() {
    #[cfg(windows)]
    assert_eq!(active_scan_backend(), ScanBackend::Win32FindFirstEx);

    #[cfg(not(windows))]
    assert_eq!(active_scan_backend(), ScanBackend::StdFs);
}

#[test]
fn std_backend_scan_is_deterministic_and_sorted() {
    let tmp = TempDir::new().expect("temp dir");

    // Create files in intentionally scrambled order
    fs::create_dir_all(tmp.path().join("x")).expect("create x");
    fs::create_dir_all(tmp.path().join("a")).expect("create a");
    fs::write(tmp.path().join("x/z.txt"), "z").expect("write x/z");
    fs::write(tmp.path().join("a/b.txt"), "b").expect("write a/b");
    fs::write(tmp.path().join("a/a.txt"), "a").expect("write a/a");
    fs::write(tmp.path().join("x/a.txt"), "xa").expect("write x/a");

    let first = scan_source_with_backend(tmp.path(), ScanBackend::StdFs).expect("scan 1");
    let second = scan_source_with_backend(tmp.path(), ScanBackend::StdFs).expect("scan 2");

    let first_paths: Vec<String> = first
        .iter()
        .map(|e| e.relative_path.to_string_lossy().to_string())
        .collect();
    let second_paths: Vec<String> = second
        .iter()
        .map(|e| e.relative_path.to_string_lossy().to_string())
        .collect();

    assert_eq!(first_paths, second_paths);
    assert_eq!(
        first_paths,
        vec!["a/a.txt", "a/b.txt", "x/a.txt", "x/z.txt"]
    );
}

#[cfg(windows)]
mod windows_filetime_tests {
    use caravan::scan::windows_filetime_ticks_to_system_time;
    use std::time::{Duration, UNIX_EPOCH};

    const WINDOWS_TO_UNIX_EPOCH_100NS: u64 = 116_444_736_000_000_000;

    #[test]
    fn windows_filetime_ticks_before_unix_epoch_returns_none() {
        assert!(windows_filetime_ticks_to_system_time(0).is_none());
        assert!(windows_filetime_ticks_to_system_time(WINDOWS_TO_UNIX_EPOCH_100NS - 1).is_none());
    }

    #[test]
    fn windows_filetime_ticks_at_unix_epoch_returns_epoch() {
        let converted = windows_filetime_ticks_to_system_time(WINDOWS_TO_UNIX_EPOCH_100NS)
            .expect("unix epoch should be representable");
        assert_eq!(converted, UNIX_EPOCH);
    }

    #[test]
    fn windows_filetime_ticks_preserve_subsecond_precision() {
        let ticks = WINDOWS_TO_UNIX_EPOCH_100NS + 1_250_000; // 125ms in 100ns units
        let converted = windows_filetime_ticks_to_system_time(ticks).expect("valid conversion");
        assert_eq!(converted, UNIX_EPOCH + Duration::from_millis(125));
    }
}
