pub const fn is_windows_build() -> bool {
    cfg!(target_os = "windows")
}

pub const fn is_linux_build() -> bool {
    cfg!(target_os = "linux")
}

pub const fn current_platform_name() -> &'static str {
    if is_windows_build() {
        "windows"
    } else {
        "linux"
    }
}
