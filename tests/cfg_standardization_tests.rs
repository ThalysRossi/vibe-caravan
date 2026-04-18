use std::fs;
use std::path::{Path, PathBuf};

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).expect("failed to read directory");
    for entry in entries {
        let entry = entry.expect("failed to read directory entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out);
            continue;
        }
        if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn normalize_cfg_scan_text(source: &str) -> String {
    source.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn has_non_standard_cfg(source: &str) -> bool {
    let normalized = normalize_cfg_scan_text(source);
    normalized.contains("#[cfg(unix)]")
        || normalized.contains("#[cfg(not(unix))]")
        || normalized.contains("#[cfg(windows)]")
        || normalized.contains("#[cfg(not(windows))]")
        || normalized.contains("#[cfg(not(target_os=\"linux\"))]")
        || normalized.contains("cfg!(windows)")
        || normalized.contains("cfg!(not(windows))")
}

#[test]
fn src_cfg_usage_is_standardized_to_linux_windows_target_os() {
    let mut rust_files = Vec::new();
    collect_rust_files(Path::new("src"), &mut rust_files);

    let mut offenders = Vec::new();
    for file in rust_files {
        let content = fs::read_to_string(&file).expect("failed to read rust source file");
        if has_non_standard_cfg(&content) {
            offenders.push(file.display().to_string());
        }
    }

    assert!(
        offenders.is_empty(),
        "found non-standard cfg usage in src files: {}",
        offenders.join(", ")
    );
}

#[test]
fn non_standard_cfg_detection_handles_whitespace_variants() {
    let cases = [
        "# [ cfg ( unix ) ]",
        "#[ cfg( not ( unix ) ) ]",
        "#[cfg( windows )]",
        "#[cfg(not( windows ))]",
        "# [ cfg ( not( target_os = \"linux\" ) ) ]",
        "cfg! ( windows )",
        "cfg! ( not ( windows ) )",
    ];

    for source in cases {
        assert!(
            has_non_standard_cfg(source),
            "expected detection for non-standard cfg pattern variant: {source}"
        );
    }
}
