use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultTarget {
    Runtime,
    Model,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    CorruptArchiveDigest,
    CorruptExtractedFile,
    CorruptInstalledFile,
    StaleVerificationManifest,
    InterruptAfterExtraction,
    InterruptAfterActivation,
    StaleDownloadingState,
}

#[derive(Debug, Clone)]
pub struct FaultScenario {
    pub target: FaultTarget,
    pub kind: FaultKind,
    pub description: String,
}

impl FaultScenario {
    pub fn all() -> Vec<Self> {
        vec![
            Self {
                target: FaultTarget::Runtime,
                kind: FaultKind::CorruptArchiveDigest,
                description: "runtime archive digest does not match catalog".into(),
            },
            Self {
                target: FaultTarget::Runtime,
                kind: FaultKind::CorruptExtractedFile,
                description: "runtime archive extracts to a corrupted DLL".into(),
            },
            Self {
                target: FaultTarget::Runtime,
                kind: FaultKind::CorruptInstalledFile,
                description: "installed runtime DLL is corrupted after activation".into(),
            },
            Self {
                target: FaultTarget::Runtime,
                kind: FaultKind::StaleVerificationManifest,
                description: "runtime verification manifest does not match installed DLL".into(),
            },
            Self {
                target: FaultTarget::Runtime,
                kind: FaultKind::InterruptAfterExtraction,
                description: "app terminates after runtime extraction before activation".into(),
            },
            Self {
                target: FaultTarget::Model,
                kind: FaultKind::CorruptArchiveDigest,
                description: "model archive digest does not match catalog".into(),
            },
            Self {
                target: FaultTarget::Model,
                kind: FaultKind::CorruptExtractedFile,
                description: "model archive extracts to a corrupted ONNX file".into(),
            },
            Self {
                target: FaultTarget::Model,
                kind: FaultKind::CorruptInstalledFile,
                description: "installed model is corrupted after successful verification".into(),
            },
            Self {
                target: FaultTarget::Model,
                kind: FaultKind::StaleVerificationManifest,
                description: "model verification manifest does not match installed ONNX".into(),
            },
            Self {
                target: FaultTarget::Model,
                kind: FaultKind::InterruptAfterExtraction,
                description: "app terminates after model write before verification manifest commit"
                    .into(),
            },
            Self {
                target: FaultTarget::Model,
                kind: FaultKind::StaleDownloadingState,
                description: "bootstrap state is Downloading while a valid managed model exists"
                    .into(),
            },
        ]
    }

    pub fn assertion_id(&self) -> String {
        let target = match self.target {
            FaultTarget::Runtime => "RUNTIME",
            FaultTarget::Model => "MODEL",
        };
        let kind = match self.kind {
            FaultKind::CorruptArchiveDigest => "CORRUPT-ARCHIVE-DIGEST",
            FaultKind::CorruptExtractedFile => "CORRUPT-EXTRACTED",
            FaultKind::CorruptInstalledFile => "CORRUPT-INSTALLED",
            FaultKind::StaleVerificationManifest => "STALE-MANIFEST",
            FaultKind::InterruptAfterExtraction => "INTERRUPT-EXTRACTION",
            FaultKind::InterruptAfterActivation => "INTERRUPT-ACTIVATION",
            FaultKind::StaleDownloadingState => "STALE-DOWNLOADING",
        };
        format!("OKA-284-FAULT-{target}-{kind}")
    }
}

/// Append a single flipped byte to a file so its digest changes.
pub fn corrupt_file(path: &Path) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let mut file = OpenOptions::new().write(true).append(true).open(path)?;
    file.write_all(&[0xff])?;
    Ok(())
}

/// Path to the local HTTP fault server root used to serve delayed, partial, or incorrect downloads.
pub fn fault_server_root(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("automation-fault-server")
}

/// Placeholder for fault-server state. Real implementation lives with the reusable workflow and local HTTP fixtures.
pub struct FaultServer;
