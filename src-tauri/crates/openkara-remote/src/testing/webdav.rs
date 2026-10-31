use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use reqwest::Url;
use tiny_http::{Header, Method as HttpMethod, Response, Server, StatusCode as HttpStatusCode};

use crate::net_policy::{build_http_client, RetryPolicy};
use crate::storage::RepositoryStorage;
use crate::testing::{ConformanceFixture, Fault};
use crate::webdav::WebDavStorage;

#[derive(Clone, Debug)]
struct StoredObject {
    bytes: Vec<u8>,
    etag: String,
}

#[derive(Default)]
struct WebDavServerState {
    directories: BTreeSet<String>,
    files: BTreeMap<String, StoredObject>,
    revisions_issued: u64,
    faults: VecDeque<Fault>,
    stale_listings: VecDeque<BTreeMap<String, StoredObject>>,
    get_requests: BTreeMap<String, usize>,
    put_requests: BTreeMap<String, usize>,
}

impl WebDavServerState {
    fn next_etag(&mut self) -> String {
        self.revisions_issued += 1;
        format!("\"webdav-etag-{}\"", self.revisions_issued)
    }

    fn put_file(&mut self, url_path: String, bytes: Vec<u8>) -> String {
        let etag = self.next_etag();
        self.ensure_parent_dirs(&url_path);
        self.files.insert(
            url_path,
            StoredObject {
                bytes,
                etag: etag.clone(),
            },
        );
        etag
    }

    fn ensure_parent_dirs(&mut self, url_path: &str) {
        let trimmed = url_path.trim_start_matches('/');
        let parts: Vec<&str> = trimmed.split('/').collect();
        let mut current = String::from("/");
        for part in parts.iter().take(parts.len().saturating_sub(1)) {
            if !part.is_empty() {
                current.push_str(part);
                current.push('/');
                self.directories.insert(current.clone());
            }
        }
    }

    fn take_precondition_failure(&mut self, rel_path: &str) -> bool {
        let Some(index) = self.faults.iter().position(
            |fault| matches!(fault, Fault::FailPrecondition { path } if path == rel_path),
        ) else {
            return false;
        };
        self.faults.remove(index);
        true
    }

    fn take_partial_write(&mut self, rel_path: &str) -> Option<u64> {
        let index = self.faults.iter().position(
            |fault| matches!(fault, Fault::PartialWrite { path, .. } if path == rel_path),
        )?;
        match self.faults.remove(index) {
            Some(Fault::PartialWrite { stored_bytes, .. }) => Some(stored_bytes),
            _ => None,
        }
    }

    fn take_interrupted_transfer(&mut self, rel_path: &str) -> Option<u64> {
        let index = self.faults.iter().position(
            |fault| matches!(fault, Fault::InterruptTransfer { path, .. } if path == rel_path),
        )?;
        match self.faults.remove(index) {
            Some(Fault::InterruptTransfer { after_bytes, .. }) => Some(after_bytes),
            _ => None,
        }
    }

    fn move_revision_after_access(&mut self, rel_path: &str) {
        let Some(index) = self.faults.iter().position(
            |fault| matches!(fault, Fault::MoveRevision { path, .. } if path == rel_path),
        ) else {
            return;
        };
        if let Some(Fault::MoveRevision { path, bytes }) = self.faults.remove(index) {
            let url_path = format!("/{}", path.trim_start_matches('/'));
            self.put_file(url_path, bytes);
        }
    }
}

pub struct TestWebDavServer {
    base_url: String,
    storage: WebDavStorage,
    state: Arc<Mutex<WebDavServerState>>,
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TestWebDavServer {
    pub fn start() -> Self {
        let server = Server::http("127.0.0.1:0").expect("bind test WebDAV server");
        let address = server.server_addr().to_ip().expect("resolve socket addr");
        let mut initial = WebDavServerState::default();
        initial.directories.insert("/".to_string());
        let state = Arc::new(Mutex::new(initial));
        let running = Arc::new(AtomicBool::new(true));
        let thread_state = Arc::clone(&state);
        let thread_running = Arc::clone(&running);

        let handle = thread::spawn(move || {
            while thread_running.load(Ordering::SeqCst) {
                match server.recv_timeout(Duration::from_millis(25)) {
                    Ok(Some(request)) => handle_request(request, &thread_state),
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        });

        let base_url = format!("http://{address}/");
        // tiny_http keeps the connection open after a truncated response, so
        // an interrupted transfer surfaces as a stalled body; short timeouts
        // turn that stall into a fast retryable error instead of hanging.
        let policy = RetryPolicy {
            connect_timeout: Duration::from_secs(2),
            read_timeout: Duration::from_millis(500),
            attempt_deadline: Duration::from_millis(500),
            ..RetryPolicy::default()
        };
        let client = build_http_client(&policy).expect("build test WebDAV client");
        let storage = WebDavStorage::new(&base_url, "openkara", "test-password")
            .expect("construct test WebDavStorage")
            .with_client(client);

        Self {
            base_url,
            storage,
            state,
            running,
            handle: Some(handle),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn storage(&self) -> &WebDavStorage {
        &self.storage
    }

    pub fn script(&self, fault: Fault) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match fault {
            Fault::StaleListing => {
                let snapshot = state.files.clone();
                state.stale_listings.push_back(snapshot);
            }
            other => state.faults.push_back(other),
        }
    }

    pub fn directory_exists(&self, path: &str) -> bool {
        let normalized = ensure_trailing_slash(&normalize_url_path(path));
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.directories.contains(&normalized)
            || state.files.keys().any(|k| k.starts_with(&normalized))
    }

    /// GET requests received for the repository-relative `path`, so tests can
    /// prove a refresh did not re-download an object.
    pub fn get_request_count(&self, path: &str) -> usize {
        let url_path = format!("/{}", path.trim_start_matches('/'));
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_requests
            .get(&url_path)
            .copied()
            .unwrap_or(0)
    }

    /// PUT requests received for the repository-relative `path`, so tests can
    /// prove a publication did not re-upload an object.
    pub fn put_request_count(&self, path: &str) -> usize {
        let url_path = format!("/{}", path.trim_start_matches('/'));
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .put_requests
            .get(&url_path)
            .copied()
            .unwrap_or(0)
    }

    pub fn file(&self, path: &str) -> Option<Vec<u8>> {
        let normalized = normalize_url_path(path);
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .files
            .get(&normalized)
            .map(|f| f.bytes.clone())
    }
}

impl ConformanceFixture for TestWebDavServer {
    fn storage(&self) -> &dyn RepositoryStorage {
        &self.storage
    }

    fn script(&self, fault: Fault) {
        TestWebDavServer::script(self, fault);
    }
}

impl Drop for TestWebDavServer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn handle_request(mut request: tiny_http::Request, state: &Arc<Mutex<WebDavServerState>>) {
    let url_path = normalize_url_path(request.url());
    let method = request.method().clone();
    let if_match = header_value(&request, "If-Match");
    let if_none_match = header_value(&request, "If-None-Match");
    let range_header = header_value(&request, "Range");
    let destination_header = header_value(&request, "Destination");

    match method {
        HttpMethod::NonStandard(ref name) if name.as_str().eq_ignore_ascii_case("MKCOL") => {
            let dir_path = ensure_trailing_slash(&url_path);
            state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .directories
                .insert(dir_path);
            let _ = request.respond(Response::empty(HttpStatusCode(201)));
        }
        HttpMethod::NonStandard(ref name) if name.as_str().eq_ignore_ascii_case("PROPFIND") => {
            handle_propfind(request, state, &url_path);
        }
        HttpMethod::NonStandard(ref name) if name.as_str().eq_ignore_ascii_case("MOVE") => {
            handle_move(request, state, &url_path, destination_header.as_deref());
        }
        HttpMethod::Head => {
            handle_head(request, state, &url_path);
        }
        HttpMethod::Get => {
            handle_get(request, state, &url_path, range_header.as_deref());
        }
        HttpMethod::Put => {
            let mut body = Vec::new();
            let _ = request.as_reader().read_to_end(&mut body);
            handle_put(
                request,
                state,
                url_path,
                body,
                if_match.as_deref(),
                if_none_match.as_deref(),
            );
        }
        HttpMethod::Delete => {
            let removed = state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .files
                .remove(&url_path)
                .is_some();
            let status = if removed { 204 } else { 404 };
            let _ = request.respond(Response::empty(HttpStatusCode(status)));
        }
        _ => {
            let _ = request.respond(Response::empty(HttpStatusCode(405)));
        }
    }
}

fn handle_head(request: tiny_http::Request, state: &Arc<Mutex<WebDavServerState>>, url_path: &str) {
    let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
    if url_path.ends_with('/') {
        let exists = guard.directories.contains(url_path)
            || guard.files.keys().any(|k| k.starts_with(url_path));
        drop(guard);
        let status = if exists { 200 } else { 404 };
        let _ = request.respond(Response::empty(HttpStatusCode(status)));
        return;
    }

    if let Some(obj) = guard.files.get(url_path).cloned() {
        let rel = rel_path(url_path);
        guard.move_revision_after_access(rel);
        drop(guard);
        let headers = vec![make_header("ETag", &obj.etag)];
        // tiny_http switches to chunked transfer encoding (no Content-Length)
        // above 32 KiB; real WebDAV servers always send Content-Length on HEAD.
        let response = Response::new(
            HttpStatusCode(200),
            headers,
            std::io::empty(),
            Some(obj.bytes.len()),
            None,
        )
        .with_chunked_threshold(usize::MAX);
        let _ = request.respond(response);
        return;
    }

    let dir_exists = guard.directories.contains(&ensure_trailing_slash(url_path));
    drop(guard);
    let status = if dir_exists { 200 } else { 404 };
    let _ = request.respond(Response::empty(HttpStatusCode(status)));
}

fn handle_get(
    request: tiny_http::Request,
    state: &Arc<Mutex<WebDavServerState>>,
    url_path: &str,
    range_header: Option<&str>,
) {
    let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
    *guard.get_requests.entry(url_path.to_string()).or_insert(0) += 1;
    let Some(obj) = guard.files.get(url_path).cloned() else {
        drop(guard);
        let _ = request.respond(Response::empty(HttpStatusCode(404)));
        return;
    };

    let rel = rel_path(url_path);
    if let Some(raw_range) = range_header {
        let total = obj.bytes.len() as u64;
        let Some((start, end)) = parse_byte_range(raw_range) else {
            drop(guard);
            let mut resp = Response::empty(HttpStatusCode(416));
            resp.add_header(make_header("Content-Range", &format!("bytes */{total}")));
            let _ = request.respond(resp);
            return;
        };
        if start >= total || start > end {
            drop(guard);
            let mut resp = Response::empty(HttpStatusCode(416));
            resp.add_header(make_header("Content-Range", &format!("bytes */{total}")));
            let _ = request.respond(resp);
            return;
        }

        let clamped_end = end.min(total - 1);
        let window = obj.bytes[start as usize..=clamped_end as usize].to_vec();
        let content_range = format!("bytes {start}-{clamped_end}/{total}");

        if let Some(after_bytes) = guard.take_interrupted_transfer(rel) {
            drop(guard);
            write_truncated_response(
                request,
                "206 Partial Content",
                &[("Content-Range", &content_range), ("ETag", &obj.etag)],
                &window,
                after_bytes,
            );
            return;
        }

        guard.move_revision_after_access(rel);
        drop(guard);
        let mut resp = Response::from_data(window).with_status_code(HttpStatusCode(206));
        resp.add_header(make_header("Content-Range", &content_range));
        resp.add_header(make_header("ETag", &obj.etag));
        let _ = request.respond(resp);
        return;
    }

    if let Some(after_bytes) = guard.take_interrupted_transfer(rel) {
        drop(guard);
        write_truncated_response(
            request,
            "200 OK",
            &[("ETag", &obj.etag)],
            &obj.bytes,
            after_bytes,
        );
        return;
    }

    guard.move_revision_after_access(rel);
    drop(guard);
    let mut resp = Response::from_data(obj.bytes);
    resp.add_header(make_header("ETag", &obj.etag));
    let _ = request.respond(resp);
}

fn handle_put(
    request: tiny_http::Request,
    state: &Arc<Mutex<WebDavServerState>>,
    url_path: String,
    body: Vec<u8>,
    if_match: Option<&str>,
    if_none_match: Option<&str>,
) {
    let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
    *guard.put_requests.entry(url_path.clone()).or_insert(0) += 1;
    let rel = rel_path(&url_path).to_string();

    if guard.take_precondition_failure(&rel) {
        drop(guard);
        let _ = request.respond(Response::empty(HttpStatusCode(412)));
        return;
    }

    if if_none_match == Some("*") && guard.files.contains_key(&url_path) {
        drop(guard);
        let _ = request.respond(Response::empty(HttpStatusCode(412)));
        return;
    }

    if let Some(expected) = if_match {
        let matches = guard
            .files
            .get(&url_path)
            .is_some_and(|existing| existing.etag == expected);
        if !matches {
            drop(guard);
            let _ = request.respond(Response::empty(HttpStatusCode(412)));
            return;
        }
    }

    if let Some(stored_bytes) = guard.take_partial_write(&rel) {
        let limit = (stored_bytes as usize).min(body.len());
        guard.put_file(url_path, body[..limit].to_vec());
        drop(guard);
        let _ = request.respond(Response::empty(HttpStatusCode(500)));
        return;
    }

    let etag = guard.put_file(url_path, body);
    drop(guard);
    let mut resp = Response::empty(HttpStatusCode(201));
    resp.add_header(make_header("ETag", &etag));
    let _ = request.respond(resp);
}

fn handle_move(
    request: tiny_http::Request,
    state: &Arc<Mutex<WebDavServerState>>,
    url_path: &str,
    destination_header: Option<&str>,
) {
    let Some(dest_raw) = destination_header else {
        let _ = request.respond(Response::empty(HttpStatusCode(400)));
        return;
    };
    let dest_path = Url::parse(dest_raw)
        .map(|u| u.path().to_string())
        .unwrap_or_else(|_| dest_raw.to_string());
    let dest_path = normalize_url_path(&dest_path);

    let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(existing) = guard.files.remove(url_path) else {
        drop(guard);
        let _ = request.respond(Response::empty(HttpStatusCode(404)));
        return;
    };
    let etag = guard.put_file(dest_path, existing.bytes);
    drop(guard);
    let mut resp = Response::empty(HttpStatusCode(201));
    resp.add_header(make_header("ETag", &etag));
    let _ = request.respond(resp);
}

fn handle_propfind(
    request: tiny_http::Request,
    state: &Arc<Mutex<WebDavServerState>>,
    url_path: &str,
) {
    let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
    let stale = guard.stale_listings.pop_front();
    let dir_prefix = ensure_trailing_slash(url_path);

    let matching: Vec<(String, StoredObject)> = {
        let source = stale.as_ref().unwrap_or(&guard.files);
        source
            .iter()
            .filter(|(k, _)| *k == url_path || k.starts_with(&dir_prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };

    if stale.is_none()
        && dir_prefix != "/"
        && !guard.directories.contains(&dir_prefix)
        && matching.is_empty()
    {
        drop(guard);
        let _ = request.respond(Response::empty(HttpStatusCode(404)));
        return;
    }
    drop(guard);

    let xml = build_propfind_xml(&dir_prefix, &matching);
    let mut resp = Response::from_data(xml.into_bytes()).with_status_code(HttpStatusCode(207));
    resp.add_header(make_header(
        "Content-Type",
        "application/xml; charset=utf-8",
    ));
    let _ = request.respond(resp);
}

fn build_propfind_xml(dir_prefix: &str, files: &[(String, StoredObject)]) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<D:multistatus xmlns:D=\"DAV:\">\n",
    );
    xml.push_str(&format!(
        "  <D:response>\n    <D:href>{}</D:href>\n    <D:propstat>\n      <D:prop>\n        <D:resourcetype><D:collection/></D:resourcetype>\n      </D:prop>\n      <D:status>HTTP/1.1 200 OK</D:status>\n    </D:propstat>\n  </D:response>\n",
        xml_escape(dir_prefix)
    ));
    for (path, obj) in files {
        xml.push_str(&format!(
            "  <D:response>\n    <D:href>{}</D:href>\n    <D:propstat>\n      <D:prop>\n        <D:resourcetype/>\n        <D:getcontentlength>{}</D:getcontentlength>\n        <D:getetag>{}</D:getetag>\n      </D:prop>\n      <D:status>HTTP/1.1 200 OK</D:status>\n    </D:propstat>\n  </D:response>\n",
            xml_escape(path),
            obj.bytes.len(),
            xml_escape(&obj.etag),
        ));
    }
    xml.push_str("</D:multistatus>\n");
    xml
}

fn xml_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Writes an HTTP response whose advertised `Content-Length` exceeds the bytes
/// delivered before closing the socket, simulating a mid-stream network drop.
fn write_truncated_response(
    request: tiny_http::Request,
    status_line: &str,
    headers: &[(&str, &str)],
    full_window: &[u8],
    after_bytes: u64,
) {
    let delivered = (after_bytes as usize).min(full_window.len());
    let advertised_len = full_window.len().max(delivered + 1);
    let mut writer = request.into_writer();
    let _ = write!(writer, "HTTP/1.1 {status_line}\r\n");
    for (k, v) in headers {
        let _ = write!(writer, "{k}: {v}\r\n");
    }
    let _ = write!(
        writer,
        "Content-Length: {advertised_len}\r\nConnection: close\r\n\r\n"
    );
    let _ = writer.write_all(&full_window[..delivered]);
    let _ = writer.flush();
}

fn header_value(request: &tiny_http::Request, name: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().to_string())
}

fn make_header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid HTTP header")
}

fn parse_byte_range(raw: &str) -> Option<(u64, u64)> {
    let rest = raw.trim().strip_prefix("bytes=")?;
    let (start_str, end_str) = rest.split_once('-')?;
    let start = start_str.trim().parse::<u64>().ok()?;
    let end = end_str.trim().parse::<u64>().ok()?;
    Some((start, end))
}

fn normalize_url_path(raw: &str) -> String {
    let without_query = raw.split('?').next().unwrap_or(raw);
    if without_query.starts_with('/') {
        without_query.to_string()
    } else {
        format!("/{without_query}")
    }
}

fn ensure_trailing_slash(path: &str) -> String {
    if path.ends_with('/') {
        path.to_string()
    } else {
        format!("{path}/")
    }
}

fn rel_path(url_path: &str) -> &str {
    url_path.trim_start_matches('/')
}
