use super::types::*;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::Path,
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
                process.stop().await;
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
pub struct RequestHandle {
    id: String,
    receiver: Option<oneshot::Receiver<HostResult<Value>>>,
    progress: broadcast::Sender<Value>,
    cancellation: Cancellation,
    first_progress: Mutex<Option<broadcast::Receiver<Value>>>,
}
impl RequestHandle {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
    pub fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }
    pub fn subscribe_progress(&self) -> broadcast::Receiver<Value> {
        self.first_progress
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| self.progress.subscribe())
    }
    pub async fn result(mut self) -> HostResult<Value> {
        self.receiver
            .take()
            .expect("request receiver consumed once")
            .await
            .unwrap_or_else(|_| Err(unavailable()))
    }
}
impl Drop for RequestHandle {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}
fn unavailable() -> HostError {
    HostError::new(
        "source_unavailable",
        "Source process stopped. Retry or update the source.",
    )
}
impl Process {
    pub(crate) async fn start(
        executable: &Path,
        package: &Path,
        release: &Release,
        config: &HostConfig,
        spawn: impl FnOnce(&mut Command) -> HostResult<Child>,
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
        let mut child = spawn(&mut command)?;
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
                            process.stop().await;
                            return;
                        }
                    }
                    Err(_) => {
                        process.stop().await;
                        return;
                    }
                }
            }
        });
        let handshake=process.send("handshake",json!({"protocol_majors":supported(config.current_protocol),"host_version":config.host_version}),true).await?;
        let result = match handshake.result().await {
            Ok(result) => result,
            Err(error) => {
                process.stop().await;
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
            process.stop().await;
            return Err(HostError::new("incompatible_plugin","Source handshake does not match the verified package. Install a compatible source update."));
        }
        Ok(process)
    }
    pub(crate) fn alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
    pub(crate) fn idle_for(&self) -> Duration {
        self.last_used.lock().unwrap().elapsed()
    }
    pub(crate) fn pending_count(&self) -> usize {
        self.pending.lock().unwrap().len()
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
    pub(crate) async fn send(
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
                process.stop().await;
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
                process.stop().await;
            }
        });
        Ok(RequestHandle {
            id,
            receiver: Some(receiver),
            progress,
            cancellation,
            first_progress: Mutex::new(Some(first_progress)),
        })
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
    pub(crate) async fn stop(&self) {
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
pub(crate) fn supported(current: u32) -> Vec<u32> {
    if current > 1 {
        vec![current, current - 1]
    } else {
        vec![1]
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
