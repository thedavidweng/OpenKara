use super::{
    install,
    runtime::{
        unavailable, Admission, BoxFuture, PreparedRuntime, Progress, RequestHandle,
        SourceInstance, SourceRuntime,
    },
    types::*,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{broadcast, oneshot},
};

const MAX_LINE: usize = 1024 * 1024;
const MISSING_RUNTIME: &str =
    "Source runtime is missing. Reinstall this source to restore verified runtime files.";
struct Pending {
    result: oneshot::Sender<HostResult<Value>>,
    progress: broadcast::Sender<Value>,
    settled: Cancellation,
    failure: Option<HostError>,
}
struct DispatchGuard {
    process: std::sync::Weak<Process>,
    id: String,
    writing: bool,
    armed: bool,
}
impl Drop for DispatchGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Some(process) = self.process.upgrade() else {
            return;
        };
        if self.writing {
            process.alive.store(false, Ordering::Release);
            tokio::spawn(async move {
                process.terminate().await;
            });
        } else if let Some(request) = process.pending.lock().unwrap().remove(&self.id) {
            request.settled.cancel();
            let _ = request.result.send(Err(HostError::new(
                "cancelled",
                "Source dispatch cancelled before transmission.",
            )));
        }
    }
}
pub(crate) struct Process {
    input: tokio::sync::Mutex<ChildStdin>,
    child: tokio::sync::Mutex<Child>,
    pending: Mutex<BTreeMap<String, Pending>>,
    sequence: AtomicU64,
    alive: AtomicBool,
    terminating: AtomicBool,
    last_used: Mutex<Instant>,
    stopped: Cancellation,
    timeout: Duration,
}
impl Process {
    pub(crate) async fn start(
        executable: &Path,
        package: &Path,
        release: &Release,
        config: &HostConfig,
        admit: Admission<'_>,
    ) -> HostResult<Arc<Self>> {
        let mut command = Command::new(executable);
        command
            .arg(package.join(&release.manifest.entry))
            .current_dir(package)
            .env_clear();
        for key in [
            "SYSTEMROOT",
            "WINDIR",
            "TEMP",
            "TMP",
            "TMPDIR",
            "HOME",
            "USERPROFILE",
            "LANG",
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let mut spawned = None;
        admit(&mut || {
            spawned = Some(command.spawn().map_err(|_| {
                HostError::new(
                    "source_unavailable",
                    "Source process cannot start. Retry or update this source.",
                )
            })?);
            Ok(())
        })?;
        let mut child: Child = spawned.ok_or_else(unavailable)?;
        let input = child.stdin.take().ok_or_else(unavailable)?;
        let output = child.stdout.take().ok_or_else(unavailable)?;
        let process = Arc::new(Self {
            input: tokio::sync::Mutex::new(input),
            child: tokio::sync::Mutex::new(child),
            pending: Mutex::new(BTreeMap::new()),
            sequence: AtomicU64::new(1),
            alive: AtomicBool::new(true),
            terminating: AtomicBool::new(false),
            last_used: Mutex::new(Instant::now()),
            stopped: Cancellation::new(),
            timeout: config.operation_timeout,
        });
        let weak = Arc::downgrade(&process);
        tokio::spawn(async move {
            let mut reader = BufReader::new(output);
            loop {
                let frame = read_frame(&mut reader).await;
                let Some(process) = weak.upgrade() else {
                    return;
                };
                match frame {
                    Ok(frame) => {
                        if process.receive(frame).is_err() {
                            process.terminate().await;
                            return;
                        }
                    }
                    Err(_) => {
                        process.terminate().await;
                        return;
                    }
                }
            }
        });
        let handshake=process.dispatch("handshake",json!({"protocol_majors":supported(config.current_protocol),"host_version":config.host_version}),true).await?;
        let result = match handshake.result().await {
            Ok(result) => result,
            Err(error) => {
                process.terminate().await;
                return Err(error);
            }
        };
        let major = result["protocol"]["major"].as_u64().unwrap_or(0) as u32;
        if !supported(config.current_protocol).contains(&major)
            || !release.manifest.protocol.majors.contains(&major)
            || result["source_id"] != release.manifest.source_id
            || result["package_id"] != release.manifest.package_id
            || result["package_version"] != release.manifest.version
        {
            process.terminate().await;
            return Err(HostError::new("incompatible_plugin","Source handshake does not match the verified package. Install a compatible source update."));
        }
        Ok(process)
    }
    async fn enqueue(
        self: &Arc<Self>,
        method: &str,
        params: Value,
        control: bool,
        deadline: Instant,
    ) -> HostResult<(
        String,
        oneshot::Receiver<HostResult<Value>>,
        broadcast::Sender<Value>,
        broadcast::Receiver<Value>,
        Cancellation,
    )> {
        if !self.alive() {
            return Err(unavailable());
        }
        let id = format!("h{}", self.sequence.fetch_add(1, Ordering::Relaxed));
        let (sender, receiver) = oneshot::channel();
        let (progress, first_progress) = broadcast::channel(32);
        let settled = Cancellation::new();
        let mut bytes = serde_json::to_vec(&json!({"id":id,"method":method,"params":params}))
            .map_err(|_| HostError::new("invalid_request", "Cannot encode source request."))?;
        bytes.push(b'\n');
        if bytes.len() > MAX_LINE {
            return Err(HostError::new(
                "invalid_request",
                "Source request exceeds the protocol size limit.",
            ));
        }
        {
            let mut pending = self.pending.lock().unwrap();
            if pending.len() >= 16 && !control {
                return Err(HostError::new(
                    "busy",
                    "Source is busy. Wait for another request to finish.",
                ));
            }
            pending.insert(
                id.clone(),
                Pending {
                    result: sender,
                    progress: progress.clone(),
                    settled: settled.clone(),
                    failure: None,
                },
            );
        }
        let mut dispatch = DispatchGuard {
            process: Arc::downgrade(self),
            id: id.clone(),
            writing: false,
            armed: true,
        };
        let write = async {
            let mut input = self.input.lock().await;
            dispatch.writing = true;
            if !self.alive() {
                return Err(unavailable());
            }
            input.write_all(&bytes).await.map_err(|_| unavailable())
        };
        tokio::time::timeout(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_secs(2)),
            write,
        )
        .await
        .map_err(|_| unavailable())??;
        dispatch.armed = false;
        *self.last_used.lock().unwrap() = Instant::now();
        Ok((id, receiver, progress, first_progress, settled))
    }
    async fn dispatch(
        self: &Arc<Self>,
        method: &str,
        params: Value,
        control: bool,
    ) -> HostResult<RequestHandle> {
        let handshake = method == "handshake";
        let timeout = if handshake {
            Duration::from_secs(10)
        } else {
            self.timeout
        };
        let deadline = Instant::now() + timeout;
        let (id, receiver, progress, first_progress, settled) =
            self.enqueue(method, params, control, deadline).await?;
        let cancellation = Cancellation::new();
        let cancel = cancellation.clone();
        let weak = Arc::downgrade(self);
        let request_id = id.clone();
        tokio::spawn(async move {
            let error = tokio::select! {
                _=settled.cancelled()=>return,
                _=cancel.cancelled()=>HostError::new("cancelled","Source request cancelled."),
                _=tokio::time::sleep(deadline.saturating_duration_since(Instant::now()))=>HostError::new("source_unavailable","Source request timed out. Retry the operation."),
            };
            let Some(process) = weak.upgrade() else {
                return;
            };
            {
                let mut pending = process.pending.lock().unwrap();
                let Some(request) = pending.get_mut(&request_id) else {
                    return;
                };
                request.failure = Some(error);
            }
            if handshake {
                process.terminate().await;
                return;
            }
            let grace = async {
                let (_, response, _, _, _) = process
                    .enqueue(
                        "request.cancel",
                        json!({"request_id":request_id}),
                        true,
                        Instant::now() + Duration::from_secs(2),
                    )
                    .await?;
                let _ = response.await;
                settled.cancelled().await;
                Ok::<(), HostError>(())
            };
            if tokio::time::timeout(Duration::from_secs(2), grace)
                .await
                .map_or(true, |result| result.is_err())
            {
                process.terminate().await;
            }
        });
        Ok(RequestHandle::new(
            id,
            receiver,
            progress,
            first_progress,
            cancellation,
        ))
    }
    fn receive(&self, frame: Value) -> HostResult<()> {
        let object = frame.as_object().ok_or_else(unavailable)?;
        let mut pending = self.pending.lock().unwrap();
        if object.get("event").and_then(Value::as_str) == Some("progress") {
            let id = frame["request_id"].as_str().ok_or_else(unavailable)?;
            let completed = frame["data"]["completed"]
                .as_u64()
                .ok_or_else(unavailable)?;
            let total = &frame["data"]["total"];
            if frame["data"]["phase"].as_str().is_none()
                || (!total.is_null() && total.as_u64().is_none_or(|total| total < completed))
            {
                return Err(unavailable());
            }
            let request = pending.get(id).ok_or_else(unavailable)?;
            let _ = request.progress.send(frame["data"].clone());
        } else {
            if object.contains_key("event")
                || object.contains_key("result") == object.contains_key("error")
            {
                return Err(unavailable());
            }
            let id = frame["id"].as_str().ok_or_else(unavailable)?;
            let request = pending.remove(id).ok_or_else(unavailable)?;
            let result = if let Some(error) = object.get("error") {
                let code = error["code"].as_str().ok_or_else(unavailable)?;
                if ![
                    "invalid_request",
                    "handshake_required",
                    "incompatible_plugin",
                    "unsupported_operation",
                    "authentication_required",
                    "session_expired",
                    "challenge_expired",
                    "invalid_cursor",
                    "not_found",
                    "network",
                    "rate_limited",
                    "busy",
                    "cancelled",
                    "source_disabled",
                    "source_unavailable",
                    "internal",
                ]
                .contains(&code)
                    || error["message"].as_str().is_none()
                    || error["retryable"].as_bool().is_none()
                {
                    return Err(unavailable());
                }
                Err(HostError::new(code,format!("Source request failed ({code}). Retry, sign in, or update the source as appropriate.")))
            } else {
                Ok(frame["result"].clone())
            };
            request.settled.cancel();
            let _ = request.result.send(request.failure.map_or(result, Err));
        }
        *self.last_used.lock().unwrap() = Instant::now();
        Ok(())
    }
    async fn terminate(&self) {
        self.alive.store(false, Ordering::Release);
        if !self.terminating.swap(true, Ordering::AcqRel) {
            self.stopped.cancel();
            let pending = std::mem::take(&mut *self.pending.lock().unwrap());
            for request in pending.into_values() {
                request.settled.cancel();
                let _ = request
                    .result
                    .send(Err(request.failure.unwrap_or_else(unavailable)));
            }
            let mut child = self.child.lock().await;
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}
fn supported(current: u32) -> Vec<u32> {
    if current > 1 {
        vec![current, current - 1]
    } else {
        vec![1]
    }
}
impl SourceInstance for Process {
    fn alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
    fn idle_for(&self) -> Duration {
        self.last_used.lock().unwrap().elapsed()
    }
    fn pending_count(&self) -> usize {
        self.pending.lock().unwrap().len()
    }
    fn send<'a>(
        self: Arc<Self>,
        method: &'a str,
        params: Value,
        control: bool,
    ) -> BoxFuture<'a, HostResult<RequestHandle>> {
        Box::pin(async move { self.dispatch(method, params, control).await })
    }
    fn stop(&self) -> BoxFuture<'_, ()> {
        Box::pin(self.terminate())
    }
}

#[derive(Default)]
pub(crate) struct NodeRuntime {
    installs: tokio::sync::Mutex<()>,
}
struct NodeLaunch {
    executable: PathBuf,
}
impl SourceRuntime for NodeRuntime {
    fn supports(&self, config: &HostConfig, release: &Release) -> bool {
        release
            .manifest
            .protocol
            .majors
            .iter()
            .any(|major| supported(config.current_protocol).contains(major))
            && release.runtime.targets.contains_key(&config.target)
            && match &config.runtime_policy {
                RuntimePolicy::Downloaded => true,
                RuntimePolicy::ChannelBundled {
                    executable,
                    version,
                    target,
                } => {
                    version == &release.runtime.version
                        && target == &config.target
                        && executable.is_file()
                }
            }
    }
    fn require_installed(&self, config: &HostConfig, release: &Release) -> HostResult<()> {
        if let RuntimePolicy::Downloaded = &config.runtime_policy {
            if !install::runtime_path(config, release)?.exists() {
                return Err(HostError::new("source_unavailable", MISSING_RUNTIME));
            }
        }
        Ok(())
    }
    fn verify_installed(&self, config: &HostConfig, release: &Release) -> HostResult<()> {
        install::verify_runtime(config, release)
    }
    fn choose_repair_slot(&self, config: &HostConfig, release: &mut Release) -> HostResult<()> {
        install::choose_runtime_slot(config, release)
    }
    fn prepare<'a>(
        &'a self,
        config: &'a HostConfig,
        release: &'a Release,
        cancel: &'a Cancellation,
        progress: Progress<'a>,
    ) -> BoxFuture<'a, HostResult<Box<dyn PreparedRuntime>>> {
        Box::pin(async move {
            let executable = {
                let _guard = self.installs.lock().await;
                install::runtime(config, release, cancel, |done| progress(done, None)).await?
            };
            Ok(Box::new(NodeLaunch { executable }) as Box<dyn PreparedRuntime>)
        })
    }
}
impl PreparedRuntime for NodeLaunch {
    fn start<'a>(
        self: Box<Self>,
        config: &'a HostConfig,
        release: &'a Release,
        package: &'a Path,
        admit: Admission<'a>,
    ) -> BoxFuture<'a, HostResult<Arc<dyn SourceInstance>>> {
        Box::pin(async move {
            let process = Process::start(&self.executable, package, release, config, admit).await?;
            Ok(process as Arc<dyn SourceInstance>)
        })
    }
}

async fn read_frame(reader: &mut BufReader<tokio::process::ChildStdout>) -> HostResult<Value> {
    let mut line = Vec::new();
    loop {
        let bytes = reader.fill_buf().await?;
        if bytes.is_empty() {
            return Err(unavailable());
        }
        let take = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |index| index + 1);
        if line.len() + take > MAX_LINE {
            return Err(unavailable());
        }
        let done = bytes[take - 1] == b'\n';
        line.extend_from_slice(&bytes[..take]);
        reader.consume(take);
        if done {
            break;
        }
    }
    serde_json::from_slice(&line).map_err(|_| unavailable())
}
