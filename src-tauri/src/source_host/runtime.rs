use super::types::*;
use serde_json::Value;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{broadcast, oneshot};

pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub(crate) type Progress<'a> = &'a (dyn Fn(u64, Option<u64>) + Send + Sync);
/// Runs the launch step only while the host still admits the release, and keeps
/// the host state locked until the step returns, so a concurrent disable or
/// replacement cannot slip between the admission check and the launch.
pub(crate) type Admission<'a> =
    &'a (dyn Fn(&mut dyn FnMut() -> HostResult<()>) -> HostResult<()> + Send + Sync);

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
const SOURCE_ERROR_CODES: &[&str] = &[
    "invalid_request",
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
];
/// Maps a protocol error code that a source reports to the host error that
/// callers see; source-supplied messages never reach callers.
pub(crate) fn source_error(code: &str) -> Option<HostError> {
    SOURCE_ERROR_CODES.contains(&code).then(|| {
        HostError::new(
            code,
            format!("Source request failed ({code}). Retry, sign in, or update the source as appropriate."),
        )
    })
}
pub(crate) fn unavailable() -> HostError {
    HostError::new(
        "source_unavailable",
        "Source stopped. Retry or update the source.",
    )
}
