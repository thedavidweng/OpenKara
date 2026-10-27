mod allocator;
mod crypto;
mod host_api;
mod log;
mod sandbox;
mod storage;

use super::{
    install,
    network::SourceNetwork,
    runtime::{
        unavailable, Admission, BoxFuture, PreparedRuntime, Progress, RequestHandle,
        SourceInstance, SourceRuntime,
    },
    types::*,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, mpsc, oneshot};

pub(crate) const RUNTIME_KIND: &str = "openkara-js";
pub(crate) const RUNTIME_MAJOR: u64 = 1;
pub(crate) const PROTOCOL_MAJOR: u32 = 2;
const MAX_CONCURRENT: usize = 16;
const STARTUP: u64 = 0;

fn stopped_at_limit() -> HostError {
    HostError::new(
        "source_unavailable",
        "Source stopped at its time or memory limit. Retry the operation.",
    )
}

struct Pending {
    deadline: Instant,
    reply: Option<oneshot::Sender<HostResult<Value>>>,
    settled: Cancellation,
}

/// State that the host side and the engine thread of one source share.
pub(super) struct Shared {
    pending: Mutex<BTreeMap<u64, Pending>>,
    stopped: AtomicBool,
    memory_exhausted: AtomicBool,
    shutdown: Cancellation,
}
impl Shared {
    fn new(startup_deadline: Instant) -> Arc<Self> {
        Arc::new(Self {
            pending: Mutex::new(BTreeMap::from([(
                STARTUP,
                Pending {
                    deadline: startup_deadline,
                    reply: None,
                    settled: Cancellation::new(),
                },
            )])),
            stopped: AtomicBool::new(false),
            memory_exhausted: AtomicBool::new(false),
            shutdown: Cancellation::new(),
        })
    }
    fn alive(&self) -> bool {
        !self.stopped.load(Ordering::Acquire)
    }
    pub(super) fn exhaust_memory(&self) {
        self.memory_exhausted.store(true, Ordering::Release);
    }
    pub(super) fn limit_reached(&self) -> bool {
        !self.alive() || self.memory_exhausted.load(Ordering::Acquire) || self.deadline_passed()
    }
    fn deadline_passed(&self) -> bool {
        let now = Instant::now();
        self.pending
            .lock()
            .unwrap()
            .values()
            .any(|pending| pending.deadline <= now)
    }
    pub(super) fn settle(&self, id: u64, result: HostResult<Value>) {
        if let Some(pending) = self.pending.lock().unwrap().remove(&id) {
            if let Some(reply) = pending.reply {
                let _ = reply.send(result);
            }
            pending.settled.cancel();
        }
    }
    fn reject(&self, id: u64, error: HostError) {
        if let Some(reply) = self
            .pending
            .lock()
            .unwrap()
            .get_mut(&id)
            .and_then(|pending| pending.reply.take())
        {
            let _ = reply.send(Err(error));
        }
    }
    pub(super) fn terminate(&self) {
        self.stopped.store(true, Ordering::Release);
        let pending = std::mem::take(&mut *self.pending.lock().unwrap());
        for request in pending.into_values() {
            if let Some(reply) = request.reply {
                let _ = reply.send(Err(stopped_at_limit()));
            }
            request.settled.cancel();
        }
        self.shutdown.cancel();
    }
}

pub(super) struct Call {
    id: u64,
    method: String,
    params: Value,
}

/// One running source: a QuickJS runtime on its own thread.
pub(crate) struct Instance {
    shared: Arc<Shared>,
    calls: mpsc::UnboundedSender<Call>,
    sequence: AtomicU64,
    last_used: Mutex<Instant>,
    describe_timeout: Duration,
    operation_timeout: Duration,
}
impl Instance {
    async fn start(
        config: &HostConfig,
        release: &Release,
        source: Vec<u8>,
        admit: Admission<'_>,
    ) -> HostResult<Arc<Self>> {
        let shared = Shared::new(Instant::now() + config.describe_timeout);
        let (calls, receiver) = mpsc::unbounded_channel();
        let (ready, loaded) = oneshot::channel();
        let mut launch = Some(sandbox::Launch {
            shared: shared.clone(),
            source,
            name: release.manifest.entry.clone(),
            memory_limit: config.engine_memory_limit,
            host: host_setup(config, release),
            calls: receiver,
            ready,
        });
        admit(&mut || {
            let launch = launch.take().ok_or_else(unavailable)?;
            std::thread::Builder::new()
                .name(format!("source-{}", release.manifest.source_id))
                .stack_size(sandbox::THREAD_STACK)
                .spawn(move || sandbox::run(launch))
                .map(|_| ())
                .map_err(|_| {
                    HostError::new(
                        "source_unavailable",
                        "Source engine cannot start. Retry or update this source.",
                    )
                })
        })?;
        let loaded = tokio::select! {
            loaded = loaded => loaded.unwrap_or_else(|_| Err(stopped_at_limit())),
            _ = tokio::time::sleep(config.describe_timeout) => Err(stopped_at_limit()),
        };
        if let Err(error) = loaded {
            shared.terminate();
            return Err(error);
        }
        shared.settle(STARTUP, Ok(Value::Null));
        let instance = Arc::new(Self {
            shared,
            calls,
            sequence: AtomicU64::new(1),
            last_used: Mutex::new(Instant::now()),
            describe_timeout: config.describe_timeout,
            operation_timeout: config.operation_timeout,
        });
        let description = instance
            .dispatch(
                "describe",
                json!({"protocol_majors":[PROTOCOL_MAJOR],"host_version":config.host_version}),
                true,
            )?
            .result()
            .await;
        let manifest = &release.manifest;
        let compatible = description.as_ref().is_ok_and(|description| {
            description["protocol"]["major"] == PROTOCOL_MAJOR
                && description["source_id"] == manifest.source_id
                && description["package_id"] == manifest.package_id
                && description["package_version"] == manifest.version
        });
        if !compatible {
            instance.shared.terminate();
            description?;
            return Err(HostError::new(
                "incompatible_plugin",
                "Source description does not match the verified package. Install a compatible source update.",
            ));
        }
        Ok(instance)
    }
    fn dispatch(&self, method: &str, params: Value, control: bool) -> HostResult<RequestHandle> {
        if !self.shared.alive() {
            return Err(unavailable());
        }
        let timeout = if method == "describe" {
            self.describe_timeout
        } else {
            self.operation_timeout
        };
        let deadline = Instant::now() + timeout;
        let id = self.sequence.fetch_add(1, Ordering::Relaxed);
        let (reply, receiver) = oneshot::channel();
        let (progress, first_progress) = broadcast::channel(1);
        let settled = Cancellation::new();
        {
            let mut pending = self.shared.pending.lock().unwrap();
            if pending.len() >= MAX_CONCURRENT && !control {
                return Err(HostError::new(
                    "busy",
                    "Source is busy. Wait for another request to finish.",
                ));
            }
            pending.insert(
                id,
                Pending {
                    deadline,
                    reply: Some(reply),
                    settled: settled.clone(),
                },
            );
        }
        let call = Call {
            id,
            method: method.into(),
            params,
        };
        if self.calls.send(call).is_err() {
            self.shared.pending.lock().unwrap().remove(&id);
            return Err(unavailable());
        }
        *self.last_used.lock().unwrap() = Instant::now();
        let cancellation = Cancellation::new();
        tokio::spawn(watch(
            Arc::downgrade(&self.shared),
            id,
            deadline,
            settled,
            cancellation.clone(),
        ));
        Ok(RequestHandle::new(
            format!("e{id}"),
            receiver,
            progress,
            first_progress,
            cancellation,
        ))
    }
}

fn host_setup(config: &HostConfig, release: &Release) -> host_api::HostSetup {
    let manifest = &release.manifest;
    let mut origins = manifest.resource_origins.clone();
    for origin in &manifest.media_origins {
        if !origins.contains(origin) {
            origins.push(origin.clone());
        }
    }
    host_api::HostSetup {
        source_id: manifest.source_id.clone(),
        owner_binding: release.owner_binding(),
        storage_root: config.data_root.clone(),
        network: SourceNetwork {
            origins,
            cookies: Default::default(),
            timeout: config.operation_timeout,
            #[cfg(test)]
            upstreams: config.fixture_upstreams.clone(),
        },
        runtime: tokio::runtime::Handle::current(),
        dispatch: tracing::dispatcher::get_default(Clone::clone),
    }
}

/// Answers a cancelled request at once and stops the source when a request
/// outlives its deadline. Cancelled work keeps its deadline until it settles.
async fn watch(
    shared: Weak<Shared>,
    id: u64,
    deadline: Instant,
    settled: Cancellation,
    cancel: Cancellation,
) {
    let mut answered = false;
    loop {
        tokio::select! {
            _ = settled.cancelled() => return,
            _ = cancel.cancelled(), if !answered => {
                answered = true;
                if let Some(shared) = shared.upgrade() {
                    shared.reject(id, HostError::new("cancelled", "Source request cancelled."));
                }
            }
            _ = tokio::time::sleep_until(deadline.into()) => {
                if let Some(shared) = shared.upgrade() {
                    shared.terminate();
                }
                return;
            }
        }
    }
}

impl SourceInstance for Instance {
    fn alive(&self) -> bool {
        self.shared.alive()
    }
    fn idle_for(&self) -> Duration {
        self.last_used.lock().unwrap().elapsed()
    }
    fn pending_count(&self) -> usize {
        self.shared.pending.lock().unwrap().len()
    }
    fn backs_off_after_stop(&self) -> bool {
        false
    }
    fn send<'a>(
        self: Arc<Self>,
        method: &'a str,
        params: Value,
        control: bool,
    ) -> BoxFuture<'a, HostResult<RequestHandle>> {
        Box::pin(async move { self.dispatch(method, params, control) })
    }
    fn stop(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move { self.shared.terminate() })
    }
}

#[derive(Default)]
pub(crate) struct EngineRuntime;
struct EngineLaunch;
impl SourceRuntime for EngineRuntime {
    fn supports(&self, _config: &HostConfig, release: &Release) -> bool {
        let manifest = &release.manifest;
        manifest.runtime.kind == RUNTIME_KIND
            && manifest.runtime.major == RUNTIME_MAJOR
            && manifest.protocol.majors.contains(&PROTOCOL_MAJOR)
    }
    fn require_installed(&self, _config: &HostConfig, _release: &Release) -> HostResult<()> {
        Ok(())
    }
    fn verify_installed(&self, _config: &HostConfig, _release: &Release) -> HostResult<()> {
        Ok(())
    }
    fn choose_repair_slot(&self, _config: &HostConfig, _release: &mut Release) -> HostResult<()> {
        Ok(())
    }
    fn prepare<'a>(
        &'a self,
        _config: &'a HostConfig,
        _release: &'a Release,
        _cancel: &'a Cancellation,
        _progress: Progress<'a>,
    ) -> BoxFuture<'a, HostResult<Box<dyn PreparedRuntime>>> {
        Box::pin(async { Ok(Box::new(EngineLaunch) as Box<dyn PreparedRuntime>) })
    }
}
impl PreparedRuntime for EngineLaunch {
    fn start<'a>(
        self: Box<Self>,
        config: &'a HostConfig,
        release: &'a Release,
        package: &'a Path,
        admit: Admission<'a>,
    ) -> BoxFuture<'a, HostResult<Arc<dyn SourceInstance>>> {
        Box::pin(async move {
            let source = std::fs::read(package.join(install::relative(&release.manifest.entry)?))?;
            let instance = Instance::start(config, release, source, admit).await?;
            Ok(instance as Arc<dyn SourceInstance>)
        })
    }
}
