use caravan::platform::{current_platform_name, is_linux_build, is_windows_build};

#[test]
fn platform_build_helpers_are_mutually_exclusive() {
    assert_ne!(
        is_windows_build(),
        is_linux_build(),
        "is_windows_build and is_linux_build must be mutually exclusive"
    );
}

#[test]
fn platform_build_helpers_match_cfg_runtime() {
    assert_eq!(is_windows_build(), cfg!(target_os = "windows"));
    assert_eq!(is_linux_build(), cfg!(target_os = "linux"));
}

#[test]
fn current_platform_name_matches_helpers() {
    let name = current_platform_name();

    if is_windows_build() {
        assert_eq!(name, "windows");
    }
    if is_linux_build() {
        assert_eq!(name, "linux");
    }
}
