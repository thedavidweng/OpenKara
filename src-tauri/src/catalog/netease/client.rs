use crate::catalog::streaming::ResolveProgress;
use crate::catalog::types::{CatalogError, StreamingCredentials};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Manager;

pub trait NeteaseApi: Send + Sync {
    fn request(
        &self,
        operation: &str,
        params: Value,
        credentials: &StreamingCredentials,
    ) -> Result<Value, CatalogError>;
    /// Streams `url` to `dest`, reporting whole-percent progress and stopping
    /// (with the partial file removed) when `progress` is cancelled.
    fn download(
        &self,
        url: &str,
        dest: &Path,
        progress: &dyn ResolveProgress,
    ) -> Result<(), CatalogError>;
}

#[derive(Default)]
pub struct NeteaseSidecar(Mutex<Option<Arc<Sidecar>>>);

type Pending = Arc<Mutex<HashMap<u64, mpsc::Sender<Result<Value, ()>>>>>;

/// Request-id multiplexer over the worker's line protocol: callers write
/// `{id, ...}` lines and a reader thread routes each `{id, ...}` response to
/// the waiting caller, so requests overlap on the one process (ADR 0035).
struct Mux {
    writer: Mutex<Box<dyn Write + Send>>,
    pending: Pending,
    next_id: AtomicU64,
    dead: Arc<AtomicBool>,
}

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(90);

impl Mux {
    fn new(writer: Box<dyn Write + Send>, reader: impl BufRead + Send + 'static) -> Self {
        let pending: Pending = Arc::default();
        let dead = Arc::new(AtomicBool::new(false));
        let (reader_pending, reader_dead) = (pending.clone(), dead.clone());
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut line = String::new();
            loop {
                line.clear();
                if !matches!(reader.read_line(&mut line), Ok(read) if read > 0) {
                    break;
                }
                let Ok(mut response) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let Some(id) = response.get("id").and_then(Value::as_u64) else {
                    continue;
                };
                if let Some(object) = response.as_object_mut() {
                    object.remove("id");
                }
                let sender = reader_pending
                    .lock()
                    .ok()
                    .and_then(|mut pending| pending.remove(&id));
                if let Some(sender) = sender {
                    let _ = sender.send(Ok(response));
                }
            }
            reader_dead.store(true, Ordering::SeqCst);
            if let Ok(mut pending) = reader_pending.lock() {
                for (_, sender) in pending.drain() {
                    let _ = sender.send(Err(()));
                }
            }
        });
        Self {
            writer: Mutex::new(writer),
            pending,
            next_id: AtomicU64::new(1),
            dead,
        }
    }

    fn is_dead(&self) -> bool {
        self.dead.load(Ordering::SeqCst)
    }

    /// Sends one request and waits for its own response. Err means the
    /// worker is unusable (write failed, exited, or timed out).
    fn roundtrip(&self, mut request: Value) -> Result<Value, ()> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        request["id"] = json!(id);
        let (sender, receiver) = mpsc::channel();
        self.pending.lock().map_err(|_| ())?.insert(id, sender);
        let written = (|| {
            let mut writer = self.writer.lock().map_err(|_| ())?;
            let mut line = serde_json::to_vec(&request).map_err(|_| ())?;
            line.push(b'\n');
            writer
                .write_all(&line)
                .and_then(|_| writer.flush())
                .map_err(|_| ())
        })();
        if written.is_err() {
            self.forget(id);
            return Err(());
        }
        match receiver.recv_timeout(RESPONSE_TIMEOUT) {
            Ok(result) => result,
            Err(_) => {
                self.forget(id);
                Err(())
            }
        }
    }

    fn forget(&self, id: u64) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&id);
        }
    }
}

struct Sidecar {
    child: Mutex<Child>,
    mux: Mux,
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
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

    /// The running worker, spawning it on first use or after it died.
    fn sidecar(&self) -> Result<Arc<Sidecar>, CatalogError> {
        let state = self.app.state::<NeteaseSidecar>();
        let mut process = state.0.lock().map_err(|_| integration_error())?;
        if let Some(sidecar) = process.as_ref().filter(|sidecar| !sidecar.mux.is_dead()) {
            return Ok(sidecar.clone());
        }
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
        let sidecar = Arc::new(Sidecar {
            child: Mutex::new(child),
            mux: Mux::new(Box::new(input), output),
        });
        *process = Some(sidecar.clone());
        Ok(sidecar)
    }

    /// Drops `sidecar` (killing it) unless a newer worker already replaced it.
    fn discard(&self, sidecar: &Arc<Sidecar>) {
        let state = self.app.state::<NeteaseSidecar>();
        let mut process = match state.0.lock() {
            Ok(process) => process,
            Err(_) => return,
        };
        if process
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, sidecar))
        {
            *process = None;
        }
    }
}

impl NeteaseApi for LiveNeteaseApi {
    fn request(
        &self,
        operation: &str,
        params: Value,
        credentials: &StreamingCredentials,
    ) -> Result<Value, CatalogError> {
        let sidecar = self.sidecar()?;
        let request = json!({ "operation": operation, "params": params, "cookie": format!("MUSIC_U={}; __csrf={}", credentials.music_u, credentials.csrf) });
        let started = Instant::now();
        let result = sidecar
            .mux
            .roundtrip(request)
            .map_err(|()| integration_error())
            .and_then(interpret_response);
        tracing::debug!(
            operation,
            elapsed_ms = started.elapsed().as_millis() as u64,
            ok = result.is_ok(),
            "netease sidecar request"
        );
        if matches!(result, Err(CatalogError::Internal(_))) {
            self.discard(&sidecar);
        }
        result
    }

    fn download(
        &self,
        url: &str,
        dest: &Path,
        progress: &dyn ResolveProgress,
    ) -> Result<(), CatalogError> {
        let response = self
            .client
            .get(url)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|_| CatalogError::Network("NetEase audio download failed".to_owned()))?;
        let total = response.content_length();
        stream_to_file(response, total, dest, progress)
    }
}

const DOWNLOAD_CHUNK_BYTES: usize = 64 * 1024;

/// Copies `reader` to `dest` in chunks. Reports a percent only when the whole
/// percent changes, checks cancellation between chunks, and removes the
/// partial file on cancel or failure.
fn stream_to_file(
    mut reader: impl Read,
    total: Option<u64>,
    dest: &Path,
    progress: &dyn ResolveProgress,
) -> Result<(), CatalogError> {
    let mut file =
        std::fs::File::create(dest).map_err(|error| CatalogError::Internal(error.to_string()))?;
    let total = total.filter(|total| *total > 0);
    let mut buffer = vec![0_u8; DOWNLOAD_CHUNK_BYTES];
    let mut received: u64 = 0;
    let mut last_percent: Option<u8> = None;
    let result = (|| {
        loop {
            if progress.is_cancelled() {
                return Err(CatalogError::Cancelled);
            }
            let read = reader
                .read(&mut buffer)
                .map_err(|_| CatalogError::Network("NetEase audio download failed".to_owned()))?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])
                .map_err(|error| CatalogError::Internal(error.to_string()))?;
            received += read as u64;
            if let Some(total) = total {
                let percent = ((received.min(total) * 100) / total) as u8;
                if last_percent != Some(percent) {
                    last_percent = Some(percent);
                    progress.downloading(Some(percent));
                }
            }
        }
        if total.is_some_and(|total| received < total) {
            return Err(CatalogError::Network(
                "NetEase audio download failed".to_owned(),
            ));
        }
        file.flush()
            .map_err(|error| CatalogError::Internal(error.to_string()))
    })();
    drop(file);
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result
}

fn interpret_response(response: Value) -> Result<Value, CatalogError> {
    if response.get("error").is_some() {
        return Err(CatalogError::Network("NetEase request failed".to_owned()));
    }
    let body = response.get("body").ok_or_else(integration_error)?.clone();
    provider_response(body)
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

#[cfg(test)]
mod download_tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Default)]
    struct Recorder {
        percents: Mutex<Vec<Option<u8>>>,
        cancelled: AtomicBool,
        cancel_after: Option<usize>,
    }

    impl ResolveProgress for Recorder {
        fn downloading(&self, percent: Option<u8>) {
            let mut percents = self.percents.lock().unwrap();
            percents.push(percent);
            if self
                .cancel_after
                .is_some_and(|limit| percents.len() >= limit)
            {
                self.cancelled.store(true, Ordering::SeqCst);
            }
        }
        fn is_cancelled(&self) -> bool {
            self.cancelled.load(Ordering::SeqCst)
        }
    }

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("openkara-dl-{name}-{}", std::process::id()))
    }

    #[test]
    fn streams_in_chunks_and_reports_each_whole_percent_once() {
        let dest = temp("ok");
        let data = vec![7_u8; DOWNLOAD_CHUNK_BYTES * 4];
        let progress = Recorder::default();
        stream_to_file(
            Cursor::new(data.clone()),
            Some(data.len() as u64),
            &dest,
            &progress,
        )
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert_eq!(
            *progress.percents.lock().unwrap(),
            vec![Some(25), Some(50), Some(75), Some(100)]
        );
        std::fs::remove_file(&dest).unwrap();
    }

    #[test]
    fn reports_nothing_when_the_length_is_unknown() {
        let dest = temp("unknown");
        let progress = Recorder::default();
        stream_to_file(Cursor::new(vec![1_u8; 10]), None, &dest, &progress).unwrap();
        assert!(progress.percents.lock().unwrap().is_empty());
        assert_eq!(std::fs::read(&dest).unwrap().len(), 10);
        std::fs::remove_file(&dest).unwrap();
    }

    #[test]
    fn cancel_mid_stream_removes_the_partial_file() {
        let dest = temp("cancel");
        let data = vec![7_u8; DOWNLOAD_CHUNK_BYTES * 4];
        let progress = Recorder {
            cancel_after: Some(2),
            ..Recorder::default()
        };
        let result = stream_to_file(
            Cursor::new(data.clone()),
            Some(data.len() as u64),
            &dest,
            &progress,
        );
        assert!(matches!(result, Err(CatalogError::Cancelled)));
        assert!(!dest.exists());
        assert_eq!(progress.percents.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_truncated_body_fails_and_removes_the_file() {
        let dest = temp("short");
        let progress = Recorder::default();
        let result = stream_to_file(Cursor::new(vec![1_u8; 10]), Some(100), &dest, &progress);
        assert!(matches!(result, Err(CatalogError::Network(_))));
        assert!(!dest.exists());
    }
}

#[cfg(test)]
mod mux_tests {
    use super::*;

    struct ChannelReader {
        lines: mpsc::Receiver<Vec<u8>>,
        buffer: Vec<u8>,
    }

    impl Read for ChannelReader {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.buffer.is_empty() {
                match self.lines.recv() {
                    Ok(bytes) => self.buffer = bytes,
                    Err(_) => return Ok(0),
                }
            }
            let n = out.len().min(self.buffer.len());
            out[..n].copy_from_slice(&self.buffer[..n]);
            self.buffer.drain(..n);
            Ok(n)
        }
    }

    struct ChannelWriter(mpsc::Sender<Vec<u8>>);

    impl Write for ChannelWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0
                .send(bytes.to_vec())
                .map_err(|_| std::io::ErrorKind::BrokenPipe)?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A worker double: reads request lines, answers them in reverse arrival
    /// order once `batch` are in, echoing each id.
    fn worker(batch: usize) -> Mux {
        let (to_worker, from_mux) = mpsc::channel::<Vec<u8>>();
        let (to_mux, from_worker) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for line in from_mux {
                let request: Value = serde_json::from_slice(&line).unwrap();
                held.push(request);
                if held.len() == batch {
                    for request in held.drain(..).rev() {
                        let reply = json!({
                            "id": request["id"],
                            "body": { "code": 200, "echo": request["params"] },
                        });
                        let mut bytes = serde_json::to_vec(&reply).unwrap();
                        bytes.push(b'\n');
                        to_mux.send(bytes).unwrap();
                    }
                }
            }
        });
        Mux::new(
            Box::new(ChannelWriter(to_worker)),
            BufReader::new(ChannelReader {
                lines: from_worker,
                buffer: Vec::new(),
            }),
        )
    }

    #[test]
    fn overlapping_requests_get_their_own_responses() {
        let mux = Arc::new(worker(3));
        let handles: Vec<_> = (0..3)
            .map(|n| {
                let mux = mux.clone();
                std::thread::spawn(move || mux.roundtrip(json!({ "params": { "n": n } })))
            })
            .collect();
        for (n, handle) in handles.into_iter().enumerate() {
            let response = handle.join().unwrap().unwrap();
            assert_eq!(response["body"]["echo"]["n"], n);
        }
    }

    #[test]
    fn a_worker_exit_fails_the_request_and_marks_the_process_dead() {
        let (writer_tx, writer_rx) = mpsc::channel::<Vec<u8>>();
        let (reader_tx, reader_rx) = mpsc::channel::<Vec<u8>>();
        let mux = Mux::new(
            Box::new(ChannelWriter(writer_tx)),
            BufReader::new(ChannelReader {
                lines: reader_rx,
                buffer: Vec::new(),
            }),
        );
        drop(writer_rx);
        drop(reader_tx);
        assert!(mux.roundtrip(json!({ "params": {} })).is_err());
        for _ in 0..100 {
            if mux.is_dead() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(mux.is_dead());
    }

    #[test]
    fn interprets_provider_and_worker_errors() {
        assert!(interpret_response(json!({ "body": { "code": 200 } })).is_ok());
        assert!(matches!(
            interpret_response(json!({ "error": "x" })),
            Err(CatalogError::Network(_))
        ));
        assert!(matches!(
            interpret_response(json!({ "body": { "code": 301 } })),
            Err(CatalogError::SessionExpired { .. })
        ));
        assert!(matches!(
            interpret_response(json!({})),
            Err(CatalogError::Internal(_))
        ));
    }
}
