pub const fn is_windows_build() -> bool {
    cfg!(target_os = "windows")
}

pub const fn is_linux_build() -> bool {
    cfg!(target_os = "linux")
}
