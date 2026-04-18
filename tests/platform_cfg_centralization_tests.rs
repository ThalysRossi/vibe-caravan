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

#[test]
fn runtime_cfg_checks_are_centralized_in_platform_module() {
    let mut rust_files = Vec::new();
    collect_rust_files(Path::new("src"), &mut rust_files);

    let mut offenders = Vec::new();
    for file in rust_files {
        let content = fs::read_to_string(&file).expect("failed to read rust source file");
        if content.contains("cfg!(") && file != Path::new("src/platform.rs") {
            offenders.push(file.display().to_string());
        }
    }

    assert!(
        offenders.is_empty(),
        "found runtime cfg!(...) outside src/platform.rs: {}",
        offenders.join(", ")
    );
}
