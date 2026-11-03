fn main() {
    // Embed git short SHA as build identifier for About dialog (H8.5).
    // Falls back to "unknown" if git is unavailable or not in a git repo.
    let git_hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=GIT_BUILD_HASH={git_hash}");

    #[cfg(target_os = "macos")]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
            println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        }
        // The AirPlay bridge is compiled by the build script, so Cargo must rerun it
        // whenever the Objective-C source changes instead of reusing a stale bridge.
        println!("cargo:rerun-if-changed=src/macos/airplay_bridge.m");
        println!("cargo:rerun-if-changed=src/macos/import_picker.m");
        println!("cargo:rerun-if-changed=src/macos/window_shell.m");

        cc::Build::new()
            .file("src/macos/airplay_bridge.m")
            .file("src/macos/import_picker.m")
            .file("src/macos/window_shell.m")
            .flag("-mmacosx-version-min=14.0")
            .flag("-fobjc-arc")
            .compile("openkara-airplay-bridge");

        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=AVKit");
        println!("cargo:rustc-link-lib=framework=AudioToolbox");
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
        println!("cargo:rustc-link-lib=framework=CoreMedia");
        println!("cargo:rustc-link-lib=framework=CoreText");
        println!("cargo:rustc-link-lib=framework=CoreVideo");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=UniformTypeIdentifiers");
    }

    let attributes = tauri_build::Attributes::new()
        .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    tauri_build::try_build(attributes).expect("failed to run tauri-build");

    // Test binaries also need Common Controls v6 to load tao's comctl32 imports.
    // Embed the manifest for every target instead of Tauri's bin-only resource.
    // See https://github.com/tauri-apps/tauri/issues/13419
    #[cfg(target_os = "windows")]
    {
        let manifest_path = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"),
        )
        .join("common-controls.manifest");
        let manifest_arg = format!("/MANIFESTINPUT:{}", manifest_path.display());

        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg={}", manifest_arg);
        println!("cargo:rerun-if-changed={}", manifest_path.display());
    }
}
