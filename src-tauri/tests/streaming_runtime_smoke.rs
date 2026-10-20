#![cfg(feature = "automation-smoke")]

use std::{fs, process::Command};

#[test]
#[ignore = "downloads the pinned runtime; run on the target OS with network access"]
fn installed_application_downloads_its_runtime_and_restarts_without_system_node() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("streaming-runtime-staging")).unwrap();
    fs::write(
        root.path()
            .join("streaming-runtime-staging/partial.download"),
        b"interrupted",
    )
    .unwrap();
    let run = |phase: &str| {
        Command::new(env!("CARGO_BIN_EXE_openkara"))
            .args(["--automation-streaming-runtime", phase])
            .arg(root.path())
            .env("PATH", "")
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .status()
            .unwrap()
    };
    assert!(run("install").success());
    let installed: serde_json::Value = serde_json::from_slice(
        &fs::read(root.path().join("streaming-runtime-install.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(installed["runtime"]["version"], "v24.21.0");
    assert_eq!(installed["runtime"]["parent_pid"], installed["app_pid"]);
    #[cfg(target_os = "macos")]
    {
        let observations = &installed["macos_quarantine"];
        for stage in [
            "downloaded_archive",
            "extracted_executable",
            "installed_executable",
        ] {
            assert_eq!(observations[stage]["quarantine_present"], false, "{stage}");
            let attributes = observations[stage]["attributes"].as_array().unwrap();
            assert!(!attributes.iter().any(|name| name == "com.apple.quarantine"));
        }
        let durable: serde_json::Value = serde_json::from_slice(
            &fs::read(
                root.path()
                    .join("streaming-runtime-quarantine-install.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(durable, *observations);
    }
    assert!(run("restart").success());
    let restarted: serde_json::Value = serde_json::from_slice(
        &fs::read(root.path().join("streaming-runtime-restart.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(restarted["runtime_sha256"], installed["runtime_sha256"]);
    assert_eq!(restarted["runtime"]["version"], "v24.21.0");
    assert_eq!(restarted["runtime"]["parent_pid"], restarted["app_pid"]);
    #[cfg(target_os = "macos")]
    {
        assert_eq!(
            restarted["macos_quarantine"]["installed_executable"]["quarantine_present"],
            false
        );
        assert_eq!(
            restarted["macos_quarantine"]["installed_executable"]["path"],
            installed["runtime_executable"]
        );
    }
}
