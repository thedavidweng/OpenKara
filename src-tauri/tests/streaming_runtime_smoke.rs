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
            .env_remove("FLATPAK_ID")
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
    assert_eq!(installed["runtime"]["version"], "v24.22.1");
    assert_eq!(installed["runtime_delivery"], "managed-download");
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
    assert_eq!(restarted["runtime"]["version"], "v24.22.1");
    assert_eq!(restarted["runtime_delivery"], "managed-download");
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

#[test]
#[ignore = "requires an installed Flatpak with its channel-bundled runtime"]
fn flatpak_reuses_its_bundled_runtime_without_installing_an_executable() {
    assert!(std::env::var_os("FLATPAK_ID").is_some());
    let root = tempfile::tempdir().unwrap();
    let mut previous_digest = None;
    for phase in ["install", "restart"] {
        let status = Command::new(env!("CARGO_BIN_EXE_openkara"))
            .args(["--automation-streaming-runtime", phase])
            .arg(root.path())
            .env("PATH", "")
            .status()
            .unwrap();
        assert!(status.success());
        let report: serde_json::Value = serde_json::from_slice(
            &fs::read(root.path().join(format!("streaming-runtime-{phase}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(report["runtime_delivery"], "channel-bundled");
        assert_eq!(
            report["runtime_executable"],
            "/app/bin/openkara-source-node"
        );
        assert_eq!(report["runtime"]["version"], "v24.22.1");
        assert_eq!(report["runtime"]["parent_pid"], report["app_pid"]);
        if let Some(digest) = &previous_digest {
            assert_eq!(&report["runtime_sha256"], digest);
        }
        previous_digest = Some(report["runtime_sha256"].clone());
    }
    let files: Vec<_> = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(files.len(), 2);
    assert!(files.contains(&"streaming-runtime-install.json".into()));
    assert!(files.contains(&"streaming-runtime-restart.json".into()));
}
