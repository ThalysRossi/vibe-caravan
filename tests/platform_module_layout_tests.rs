use std::path::Path;

#[test]
fn scan_module_is_split_into_platform_submodules() {
    assert!(
        Path::new("src/scan/mod.rs").exists(),
        "missing src/scan/mod.rs"
    );
    assert!(
        Path::new("src/scan/windows.rs").exists(),
        "missing src/scan/windows.rs"
    );
    assert!(
        Path::new("src/scan/linux.rs").exists(),
        "missing src/scan/linux.rs"
    );
    assert!(
        !Path::new("src/scan.rs").exists(),
        "legacy flat module src/scan.rs should be replaced"
    );
}

#[test]
fn preflight_module_is_split_into_platform_submodules() {
    assert!(
        Path::new("src/preflight/mod.rs").exists(),
        "missing src/preflight/mod.rs"
    );
    assert!(
        Path::new("src/preflight/platform_windows.rs").exists(),
        "missing src/preflight/platform_windows.rs"
    );
    assert!(
        Path::new("src/preflight/platform_linux.rs").exists(),
        "missing src/preflight/platform_linux.rs"
    );
    assert!(
        !Path::new("src/preflight.rs").exists(),
        "legacy flat module src/preflight.rs should be replaced"
    );
}

#[test]
fn capacity_module_is_split_into_platform_submodules() {
    assert!(
        Path::new("src/capacity/mod.rs").exists(),
        "missing src/capacity/mod.rs"
    );
    assert!(
        Path::new("src/capacity/windows.rs").exists(),
        "missing src/capacity/windows.rs"
    );
    assert!(
        Path::new("src/capacity/linux.rs").exists(),
        "missing src/capacity/linux.rs"
    );
    assert!(
        !Path::new("src/capacity.rs").exists(),
        "legacy flat module src/capacity.rs should be replaced"
    );
}
