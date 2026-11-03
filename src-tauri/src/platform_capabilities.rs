pub const fn xnnpack_available() -> bool {
    cfg!(any(
        all(target_os = "macos", target_arch = "x86_64"),
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))
}
