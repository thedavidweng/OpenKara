use crate::commands::bootstrap::{self, ModelBootstrapStatusSnapshot};
use crate::commands::error::{state_lock_error, CommandError};
use crate::commands::runtime_bootstrap::RuntimeBootstrapStatusSnapshot;
use crate::library::error::LibraryError;
use crate::library_root::LibraryRoot;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AppShell {
    pub library: Arc<Mutex<Option<LibraryRoot>>>,
    pub app_data_dir: PathBuf,
    pub app_resource_dir: PathBuf,
    pub model_path: PathBuf,
    pub model_bootstrap_status: Arc<Mutex<ModelBootstrapStatusSnapshot>>,
    pub runtime_bootstrap_status: Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    pub shutdown: Arc<AtomicBool>,
}

impl AppShell {
    pub fn new(
        library: Arc<Mutex<Option<LibraryRoot>>>,
        app_data_dir: PathBuf,
        app_resource_dir: PathBuf,
        model_path: PathBuf,
        model_bootstrap_status: Arc<Mutex<ModelBootstrapStatusSnapshot>>,
        runtime_bootstrap_status: Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    ) -> Self {
        Self {
            library,
            app_data_dir,
            app_resource_dir,
            model_path,
            model_bootstrap_status,
            runtime_bootstrap_status,
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn library_root(&self) -> Result<LibraryRoot, CommandError> {
        let guard = self
            .library
            .lock()
            .map_err(|_| state_lock_error("library lock was poisoned"))?;
        guard.clone().ok_or_else(|| {
            CommandError::from(LibraryError::Internal("no library configured".to_owned()))
        })
    }

    /// Resolve the path to the active AI model based on the current config.
    /// Checks (in order): managed model dir for the active variant, then dev fallback.
    ///
    /// This must stay variant-aware. Falling back to a single hard-coded model
    /// filename can silently run the wrong separator after users switch quality modes.
    pub fn resolve_model_path(&self) -> Result<PathBuf, CommandError> {
        let variant = crate::config::load_config(&self.app_data_dir)
            .ok()
            .flatten()
            .map(|c| c.effective_model_variant())
            .unwrap_or_default();
        let descriptor = crate::separator::bootstrap::descriptor_for(variant);
        let managed =
            crate::separator::bootstrap::managed_model_path_for(&self.app_data_dir, descriptor);
        let dev_path =
            crate::separator::model::default_model_path_for_filename(descriptor.filename);
        match crate::separator::bootstrap::resolve_model_installation(&managed, &dev_path)
            .map_err(|error| crate::commands::error::internal_error(error.to_string()))?
        {
            crate::separator::bootstrap::ModelInstallationResolution::Ready(resolved) => {
                Ok(resolved.path)
            }
            crate::separator::bootstrap::ModelInstallationResolution::Absent => {
                Err(crate::commands::error::model_bootstrap_error(
                    "model is not installed or is still downloading".to_string(),
                ))
            }
        }
    }

    pub fn test_fixture() -> Self {
        Self::new(
            Arc::new(Mutex::new(None)),
            PathBuf::from("/tmp/test-app-data"),
            PathBuf::from("/tmp/test-resources"),
            PathBuf::from("/tmp/test-models"),
            Arc::new(Mutex::new(bootstrap::pending_status("test-model.bin"))),
            Arc::new(Mutex::new(
                crate::commands::runtime_bootstrap::RuntimeBootstrapStatusSnapshot {
                    state: crate::commands::runtime_bootstrap::RuntimeBootstrapState::Missing,
                    runtime_path: "/tmp/test-app-data/runtime/test.dylib".to_owned(),
                    downloaded_bytes: None,
                    total_bytes: None,
                    version: "test".to_owned(),
                    error: None,
                },
            )),
        )
    }
}
