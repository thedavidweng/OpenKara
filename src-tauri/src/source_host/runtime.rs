use super::types::*;
use serde_json::Value;
use std::{
    future::Future,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{broadcast, oneshot};

pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub(crate) type Progress<'a> = &'a (dyn Fn(u64, Option<u64>) + Send + Sync);
/// Runs the launch step only while the host still admits the release, and keeps
/// the host state locked until the step returns, so a concurrent revocation or
/// disable cannot slip between the admission check and the launch.
pub(crate) type Admission<'a> =
    &'a (dyn Fn(&mut dyn FnMut() -> HostResult<()>) -> HostResult<()> + Send + Sync);

pub(crate) trait SourceRuntime: Send + Sync {
    fn supports(&self, config: &HostConfig, release: &Release) -> bool;
    /// Rejects a start when runtime files that only an install provides are absent.
    fn require_installed(&self, config: &HostConfig, release: &Release) -> HostResult<()>;
    fn verify_installed(&self, config: &HostConfig, release: &Release) -> HostResult<()>;
    fn choose_repair_slot(&self, config: &HostConfig, release: &mut Release) -> HostResult<()>;
    fn prepare<'a>(
        &'a self,
        config: &'a HostConfig,
        release: &'a Release,
        cancel: &'a Cancellation,
        progress: Progress<'a>,
    ) -> BoxFuture<'a, HostResult<Box<dyn PreparedRuntime>>>;
}
pub(crate) trait PreparedRuntime: Send + 'static {
    fn start<'a>(
        self: Box<Self>,
        config: &'a HostConfig,
        release: &'a Release,
        package: &'a Path,
        admit: Admission<'a>,
    ) -> BoxFuture<'a, HostResult<Arc<dyn SourceInstance>>>;
}
pub(crate) trait SourceInstance: Send + Sync + 'static {
    fn alive(&self) -> bool;
    fn idle_for(&self) -> Duration;
    fn pending_count(&self) -> usize;
    fn send<'a>(
        self: Arc<Self>,
        method: &'a str,
        params: Value,
        control: bool,
    ) -> BoxFuture<'a, HostResult<RequestHandle>>;
    fn stop(&self) -> BoxFuture<'_, ()>;
}

pub struct RequestHandle {
    id: String,
    receiver: Option<oneshot::Receiver<HostResult<Value>>>,
    progress: broadcast::Sender<Value>,
    cancellation: Cancellation,
    first_progress: Mutex<Option<broadcast::Receiver<Value>>>,
}
impl RequestHandle {
    pub(crate) fn new(
        id: String,
        receiver: oneshot::Receiver<HostResult<Value>>,
        progress: broadcast::Sender<Value>,
        first_progress: broadcast::Receiver<Value>,
        cancellation: Cancellation,
    ) -> Self {
        Self {
            id,
            receiver: Some(receiver),
            progress,
            cancellation,
            first_progress: Mutex::new(Some(first_progress)),
        }
    }
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
pub(crate) fn unavailable() -> HostError {
    HostError::new(
        "source_unavailable",
        "Source process stopped. Retry or update the source.",
    )
}
