use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};

pub const LOG_FILENAME_PREFIX: &str = "openkara";
pub const LOG_FILENAME_SUFFIX: &str = "log";
pub const REQUEST_ID_HEADER: &str = "x-request-id";

const MAX_LOG_FILES: usize = 7;
const LOG_FILTER_ENV: &str = "OPENKARA_LOG";
const DEFAULT_FILTER: &str = "info";

thread_local! {
    static ACTIVE_TRACE_ID: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Carries a request correlation `trace_id` and `tracing::Span` across IPC
/// entry points, worker threads, and outbound requests.
#[derive(Clone, Debug)]
pub struct RequestTrace {
    trace_id: String,
    span: tracing::Span,
}

struct TraceIdRestoreGuard {
    previous: Option<String>,
}

impl Drop for TraceIdRestoreGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        ACTIVE_TRACE_ID.with(|slot| {
            *slot.borrow_mut() = previous;
        });
    }
}

impl RequestTrace {
    pub fn start(operation: &'static str) -> Self {
        let trace_id = current_trace_id().unwrap_or_else(new_trace_id);
        let span = tracing::info_span!("request", operation, trace_id = %trace_id);
        Self { trace_id, span }
    }

    pub fn trace_id(&self) -> &str {
        &self.trace_id
    }

    pub fn span(&self) -> &tracing::Span {
        &self.span
    }

    pub fn in_scope<T>(&self, operation: impl FnOnce() -> T) -> T {
        let previous =
            ACTIVE_TRACE_ID.with(|slot| slot.borrow_mut().replace(self.trace_id.clone()));
        let _restore = TraceIdRestoreGuard { previous };
        self.span.in_scope(operation)
    }
}

pub fn new_trace_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn current_trace_id() -> Option<String> {
    ACTIVE_TRACE_ID.with(|slot| slot.borrow().clone())
}

pub fn active_or_new_request_id() -> String {
    current_trace_id().unwrap_or_else(new_trace_id)
}

pub fn log_file_hint(log_dir: &Path) -> PathBuf {
    log_dir.join(format!(
        "{LOG_FILENAME_PREFIX}.<date>.{LOG_FILENAME_SUFFIX}"
    ))
}

fn default_filter() -> EnvFilter {
    EnvFilter::try_from_env(LOG_FILTER_ENV)
        .or_else(|_| EnvFilter::try_new(DEFAULT_FILTER))
        .unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER))
}

pub fn init(log_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(log_dir)?;

    let file_appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILENAME_PREFIX)
        .filename_suffix(LOG_FILENAME_SUFFIX)
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)?;

    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_writer(file_appender);

    let stderr_layer = fmt::layer().with_writer(std::io::stderr);

    tracing_subscriber::registry()
        .with(default_filter())
        .with(file_layer)
        .with(stderr_layer)
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to install tracing subscriber: {error}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("log buffer lock")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn log_file_hint_joins_prefix_and_suffix() {
        let hint = log_file_hint(Path::new("/var/logs/openkara"));
        assert_eq!(
            hint,
            PathBuf::from("/var/logs/openkara/openkara.<date>.log")
        );
    }

    #[test]
    fn default_filter_is_constructible() {
        let _ = default_filter();
    }

    #[test]
    fn request_trace_propagates_trace_id_across_threads_and_formats_span_context() {
        let buffer = SharedBuf::default();
        let writer_buf = buffer.clone();
        let subscriber = tracing_subscriber::registry().with(
            fmt::layer()
                .with_ansi(false)
                .with_span_events(FmtSpan::CLOSE)
                .with_writer(move || writer_buf.clone()),
        );

        tracing::subscriber::with_default(subscriber, || {
            assert!(current_trace_id().is_none());
            let trace = RequestTrace::start("separate_song");
            let expected_id = trace.trace_id().to_owned();

            let worker_dispatch = tracing::dispatcher::get_default(Clone::clone);
            let worker_trace = trace.clone();
            let observed = std::thread::spawn(move || {
                tracing::dispatcher::with_default(&worker_dispatch, || {
                    worker_trace.in_scope(|| {
                        tracing::info!("worker started inference");
                        current_trace_id()
                    })
                })
            })
            .join()
            .expect("worker thread should join");

            assert_eq!(observed.as_deref(), Some(expected_id.as_str()));
            assert!(current_trace_id().is_none());
            drop(trace);

            let rendered =
                String::from_utf8(buffer.0.lock().expect("log buffer").clone()).expect("utf8 logs");
            assert!(
                rendered.contains(&expected_id),
                "logs missing trace_id: {rendered}"
            );
            assert!(
                rendered.contains("separate_song"),
                "logs missing operation: {rendered}"
            );
            assert!(rendered.contains("worker started inference"));
        });
    }
}
