use crate::catalog::types::{CatalogError, StreamingCredentials};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use tauri::Manager;

pub trait NeteaseApi: Send + Sync {
    fn request(
        &self,
        operation: &str,
        params: Value,
        credentials: &StreamingCredentials,
    ) -> Result<Value, CatalogError>;
    fn download(&self, url: &str, dest: &Path) -> Result<(), CatalogError>;
}

#[derive(Default)]
pub struct NeteaseSidecar(Mutex<Option<Sidecar>>);

struct Sidecar {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct LiveNeteaseApi {
    app: tauri::AppHandle,
    client: Client,
}

fn integration_error() -> CatalogError {
    CatalogError::Internal("NetEase sidecar failed. Reinstall OpenKara and try again.".to_owned())
}

impl LiveNeteaseApi {
    pub fn new(app: &tauri::AppHandle) -> Result<Self, CatalogError> {
        Ok(Self {
            app: app.clone(),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .map_err(|_| integration_error())?,
        })
    }
}

impl NeteaseApi for LiveNeteaseApi {
    fn request(
        &self,
        operation: &str,
        params: Value,
        credentials: &StreamingCredentials,
    ) -> Result<Value, CatalogError> {
        let state = self.app.state::<NeteaseSidecar>();
        let mut process = state.0.lock().map_err(|_| integration_error())?;
        if process.is_none() {
            let executable = std::env::current_exe().map_err(|_| integration_error())?;
            let node = executable
                .parent()
                .ok_or_else(integration_error)?
                .join(if cfg!(windows) {
                    "netease-node.exe"
                } else {
                    "netease-node"
                });
            let app_state = self.app.state::<crate::AppState>();
            let resources = &app_state.shell.app_resource_dir;
            let mut command = Command::new(node);
            command
                .arg(resources.join("netease/worker.mjs"))
                .current_dir(resources.join("netease"))
                .env_remove("NODE_OPTIONS")
                .env_remove("NODE_PATH")
                .env("DOTENV_CONFIG_QUIET", "true")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x0800_0000);
            }
            let mut child = command.spawn().map_err(|_| integration_error())?;
            let input = child.stdin.take().ok_or_else(integration_error)?;
            let output = BufReader::new(child.stdout.take().ok_or_else(integration_error)?);
            *process = Some(Sidecar {
                child,
                input,
                output,
            });
        }
        let sidecar = process.as_mut().ok_or_else(integration_error)?;
        let request = json!({ "operation": operation, "params": params, "cookie": format!("MUSIC_U={}; __csrf={}", credentials.music_u, credentials.csrf) });
        let result = (|| {
            serde_json::to_writer(&mut sidecar.input, &request).map_err(|_| integration_error())?;
            sidecar
                .input
                .write_all(b"\n")
                .and_then(|_| sidecar.input.flush())
                .map_err(|_| integration_error())?;
            let mut line = String::new();
            sidecar
                .output
                .read_line(&mut line)
                .map_err(|_| integration_error())?;
            let response: Value = serde_json::from_str(&line).map_err(|_| integration_error())?;
            if response.get("error").is_some() {
                return Err(CatalogError::Network("NetEase request failed".to_owned()));
            }
            let body = response.get("body").ok_or_else(integration_error)?.clone();
            provider_response(body)
        })();
        if matches!(result, Err(CatalogError::Internal(_))) {
            *process = None;
        }
        result
    }

    fn download(&self, url: &str, dest: &Path) -> Result<(), CatalogError> {
        let mut response = self
            .client
            .get(url)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|_| CatalogError::Network("NetEase audio download failed".to_owned()))?;
        let mut file = std::fs::File::create(dest)
            .map_err(|error| CatalogError::Internal(error.to_string()))?;
        if response.copy_to(&mut file).is_err() {
            drop(file);
            let _ = std::fs::remove_file(dest);
            return Err(CatalogError::Network(
                "NetEase audio download failed".to_owned(),
            ));
        }
        Ok(())
    }
}

pub fn provider_response(body: Value) -> Result<Value, CatalogError> {
    match body.get("code").and_then(Value::as_i64) {
        Some(200) => Ok(body),
        Some(301) => Err(CatalogError::SessionExpired {
            source_id: "netease".to_owned(),
        }),
        Some(code) => Err(CatalogError::AuthFailed {
            source_id: "netease".to_owned(),
            detail: format!("NetEase rejected the request (code {code})"),
        }),
        None => Err(integration_error()),
    }
}
