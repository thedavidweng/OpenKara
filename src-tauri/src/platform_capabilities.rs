#[cfg(target_os = "macos")]
pub const fn coreml_available() -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
pub const fn coreml_available() -> bool {
    false
}

pub const fn xnnpack_available() -> bool {
    cfg!(any(
        all(target_os = "macos", target_arch = "x86_64"),
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))
}
