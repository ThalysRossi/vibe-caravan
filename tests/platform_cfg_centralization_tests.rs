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

fn has_runtime_cfg_macro(source: &str) -> bool {
    normalize_cfg_scan_text(source).contains("cfg!(")
}

#[test]
fn runtime_cfg_checks_are_centralized_in_platform_module() {
    let mut rust_files = Vec::new();
    collect_rust_files(Path::new("src"), &mut rust_files);

    let mut offenders = Vec::new();
    for file in rust_files {
        let content = fs::read_to_string(&file).expect("failed to read rust source file");
        if has_runtime_cfg_macro(&content) && file != Path::new("src/platform.rs") {
            offenders.push(file.display().to_string());
        }
    }

    assert!(
        offenders.is_empty(),
        "found runtime cfg!(...) outside src/platform.rs: {}",
        offenders.join(", ")
    );
}

#[test]
fn runtime_cfg_detection_handles_whitespace_variants() {
    let cases = [
        "cfg! ( target_os = \"windows\" )",
        "cfg! ( target_os = \"linux\" )",
    ];
    for source in cases {
        assert!(
            has_runtime_cfg_macro(source),
            "expected runtime cfg!(...) detection for variant: {source}"
        );
    }
}
