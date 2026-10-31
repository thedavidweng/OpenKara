use std::io::Write;
use std::str::FromStr;

use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::blocking::{Client, Response};
use reqwest::{Method, StatusCode, Url};

use crate::error::{
    kind_from_io_error, verify_content_range, RemoteError, RemoteErrorKind, RemoteObjectMetadata,
    RemoteResult,
};
use crate::net_policy::{
    build_http_client, classify_reqwest_error, classify_status, parse_retry_after,
    remote_error_with_retry_after, stream_response_body, RetryPolicy,
};
use crate::storage::{
    is_under_prefix, validate_object_path, validate_prefix, ListedObject, ObjectMetadata,
    RepositoryStorage, StorageCapabilities,
};

const PROPFIND_REQUEST_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:">
  <D:prop>
    <D:resourcetype/>
    <D:getcontentlength/>
    <D:getetag/>
    <D:getlastmodified/>
  </D:prop>
</D:propfind>"#;

#[derive(Clone, Debug)]
pub struct WebDavStorage {
    client: Client,
    server_url: String,
    root_url: String,
    username: String,
    password: String,
    capabilities: StorageCapabilities,
}

impl WebDavStorage {
    pub fn new(
        root_url: &str,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> RemoteResult<Self> {
        let normalized_root = normalize_server_url(root_url)?;
        Self::with_server_and_root(&normalized_root, &normalized_root, username, password)
    }

    pub fn with_server_and_root(
        server_url: &str,
        root_url: &str,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> RemoteResult<Self> {
        let normalized_server = normalize_server_url(server_url)?;
        let normalized_root = normalize_server_url(root_url)?;
        let client = webdav_client()?;
        Ok(Self {
            client,
            server_url: normalized_server,
            root_url: normalized_root,
            username: username.into(),
            password: password.into(),
            capabilities: StorageCapabilities {
                conditional_replace: true,
                range_read: true,
            },
        })
    }

    pub fn with_capabilities(mut self, capabilities: StorageCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn with_client(mut self, client: Client) -> Self {
        self.client = client;
        self
    }

    pub fn root_url(&self) -> &str {
        &self.root_url
    }

    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    fn object_url(&self, path: &str) -> RemoteResult<String> {
        validate_object_path(path)?;
        join_url(&self.root_url, path)
    }
}

impl RepositoryStorage for WebDavStorage {
    fn capabilities(&self) -> StorageCapabilities {
        self.capabilities
    }

    fn stat(&self, path: &str) -> RemoteResult<Option<ObjectMetadata>> {
        let url = self.object_url(path)?;
        let meta = webdav_stat(&self.client, &url, &self.username, &self.password)?;
        Ok(meta.map(|m| ObjectMetadata {
            size_bytes: m.size_bytes.unwrap_or(0),
            revision: m.revision,
        }))
    }

    fn read(&self, path: &str, sink: &mut dyn Write) -> RemoteResult<u64> {
        let url = self.object_url(path)?;
        let mut response = webdav_send(
            &self.client,
            Method::GET,
            &url,
            &self.username,
            &self.password,
            None,
            None,
        )?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Err(RemoteError::new(
                RemoteErrorKind::ObjectNotFound,
                format!("WebDAV object not found: {path}"),
            ));
        }
        if !status.is_success() {
            return Err(error_from_response(&response, "WebDAV GET"));
        }

        let expected_len = content_length_header(&response);
        let written = stream_response_body(&mut response, sink)
            .map_err(|err| RemoteError::new(kind_from_io_error(&err), err.to_string()))?;

        if let Some(expected) = expected_len {
            if written < expected {
                return Err(RemoteError::new(
                    RemoteErrorKind::NetworkUnavailable,
                    format!("WebDAV read truncated after {written} of {expected} bytes"),
                ));
            }
        }
        let _ = sink.flush();
        Ok(written)
    }

    fn read_range(
        &self,
        path: &str,
        offset: u64,
        length: u64,
        sink: &mut dyn Write,
    ) -> RemoteResult<u64> {
        validate_object_path(path)?;
        if !self.capabilities.range_read {
            return Err(RemoteError::new(
                RemoteErrorKind::ProviderCapabilityUnavailable,
                "storage does not support range reads",
            ));
        }
        if length == 0 {
            return Ok(0);
        }

        let url = self.object_url(path)?;
        let end = offset.saturating_add(length).saturating_sub(1);
        let range_header = format!("bytes={offset}-{end}");
        let mut response = self
            .client
            .get(&url)
            .basic_auth(&self.username, Some(&self.password))
            .header(reqwest::header::RANGE, &range_header)
            .send()
            .map_err(|err| {
                RemoteError::new(
                    classify_reqwest_error(&err),
                    format!("WebDAV range GET failed: {err}"),
                )
            })?;

        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Err(RemoteError::new(
                RemoteErrorKind::ObjectNotFound,
                format!("WebDAV object not found: {path}"),
            ));
        }
        if status == StatusCode::RANGE_NOT_SATISFIABLE {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!("WebDAV range {range_header} is not satisfiable for {path}"),
            ));
        }
        if status != StatusCode::PARTIAL_CONTENT {
            return Err(error_from_response(&response, "WebDAV range GET"));
        }

        let content_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                RemoteError::new(
                    RemoteErrorKind::RemoteIntegrityFailed,
                    "WebDAV 206 response is missing Content-Range header",
                )
            })?
            .to_string();
        verify_content_range(&content_range, offset, length)?;
        let expected_window_len = content_range_window_len(&content_range)?;

        let written = stream_response_body(&mut response, sink)
            .map_err(|err| RemoteError::new(kind_from_io_error(&err), err.to_string()))?;
        if written < expected_window_len {
            return Err(RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!(
                    "WebDAV range read truncated after {written} of {expected_window_len} bytes"
                ),
            ));
        }
        if written > expected_window_len {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!(
                    "WebDAV range read delivered {written} bytes, expected {expected_window_len}"
                ),
            ));
        }
        let _ = sink.flush();
        Ok(written)
    }

    fn write_new(&self, path: &str, bytes: &[u8]) -> RemoteResult<ObjectMetadata> {
        let url = self.object_url(path)?;
        ensure_webdav_collection_chain(
            &self.client,
            &self.server_url,
            &url,
            &self.username,
            &self.password,
        )?;
        let written_len = bytes.len() as u64;
        let meta = webdav_conditional_put(
            &self.client,
            &url,
            bytes.to_vec(),
            &self.username,
            &self.password,
            None,
        )?;
        Ok(ObjectMetadata {
            size_bytes: written_len,
            revision: meta.revision,
        })
    }

    fn conditional_replace(
        &self,
        path: &str,
        bytes: &[u8],
        expected_revision: &str,
    ) -> RemoteResult<ObjectMetadata> {
        validate_object_path(path)?;
        if !self.capabilities.conditional_replace {
            return Err(RemoteError::new(
                RemoteErrorKind::ProviderCapabilityUnavailable,
                "storage does not support conditional replacement",
            ));
        }
        let url = self.object_url(path)?;
        let written_len = bytes.len() as u64;
        let meta = webdav_conditional_put(
            &self.client,
            &url,
            bytes.to_vec(),
            &self.username,
            &self.password,
            Some(expected_revision),
        )?;
        Ok(ObjectMetadata {
            size_bytes: written_len,
            revision: meta.revision,
        })
    }

    fn list(&self, prefix: &str) -> RemoteResult<Vec<ListedObject>> {
        validate_prefix(prefix)?;
        let target_url = if prefix.is_empty() {
            self.root_url.clone()
        } else {
            join_url(&self.root_url, &format!("{prefix}/"))?
        };
        let propfind = Method::from_str("PROPFIND").map_err(|err| {
            RemoteError::new(
                RemoteErrorKind::NetworkUnavailable,
                format!("failed to construct PROPFIND method: {err}"),
            )
        })?;
        let mut response = self
            .client
            .request(propfind, &target_url)
            .basic_auth(&self.username, Some(&self.password))
            .header("Depth", "infinity")
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/xml; charset=utf-8",
            )
            .body(PROPFIND_REQUEST_BODY)
            .send()
            .map_err(|err| {
                RemoteError::new(
                    classify_reqwest_error(&err),
                    format!("WebDAV PROPFIND failed: {err}"),
                )
            })?;

        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        if status != StatusCode::MULTI_STATUS && !status.is_success() {
            return Err(error_from_response(&response, "WebDAV PROPFIND"));
        }

        let mut body = Vec::new();
        stream_response_body(&mut response, &mut body)
            .map_err(|err| RemoteError::new(kind_from_io_error(&err), err.to_string()))?;
        let xml = String::from_utf8(body).map_err(|err| {
            RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!("WebDAV PROPFIND response is not valid UTF-8: {err}"),
            )
        })?;

        parse_propfind_xml(&xml, &self.root_url, prefix)
    }

    fn delete(&self, path: &str) -> RemoteResult<()> {
        let url = self.object_url(path)?;
        let response = webdav_send(
            &self.client,
            Method::DELETE,
            &url,
            &self.username,
            &self.password,
            None,
            None,
        )?;
        let status = response.status();
        if status.is_success() || status == StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(error_from_response(&response, "WebDAV DELETE"))
        }
    }
}

pub fn normalize_server_url(raw: &str) -> RemoteResult<String> {
    let mut parsed = Url::parse(raw.trim()).map_err(|error| {
        RemoteError::new(
            RemoteErrorKind::PermissionDenied,
            format!("invalid WebDAV server URL: {error}"),
        )
    })?;
    if !parsed.path().ends_with('/') {
        let updated = format!("{}/", parsed.path());
        parsed.set_path(&updated);
    }
    Ok(parsed.to_string())
}

pub fn normalize_webdav_root_path(raw: Option<&str>, fallback_display_name: &str) -> String {
    let candidate = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            let slug = fallback_display_name
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                        ch
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
                .trim_matches('-')
                .to_string();
            if slug.is_empty() {
                "OpenKara".to_string()
            } else {
                format!("/{slug}")
            }
        });

    let trimmed = candidate.trim_matches('/');
    if trimmed.is_empty() {
        "/OpenKara".to_string()
    } else {
        format!("/{trimmed}")
    }
}

pub fn join_url(base: &str, relative: &str) -> RemoteResult<String> {
    let parsed_base = Url::parse(base).map_err(|error| {
        RemoteError::new(
            RemoteErrorKind::PermissionDenied,
            format!("invalid base URL {base}: {error}"),
        )
    })?;
    parsed_base
        .join(relative.trim_start_matches('/'))
        .map(|url| url.to_string())
        .map_err(|error| {
            RemoteError::new(
                RemoteErrorKind::PermissionDenied,
                format!("failed to join URL {base} + {relative}: {error}"),
            )
        })
}

pub fn remote_path_display_from_url(url: &str) -> String {
    Url::parse(url)
        .map(|parsed| {
            format!(
                "{}{}",
                parsed.host_str().unwrap_or("webdav"),
                parsed.path().trim_end_matches('/')
            )
        })
        .unwrap_or_else(|_| url.to_string())
}

pub fn webdav_client() -> RemoteResult<Client> {
    build_http_client(&RetryPolicy::default())
        .map_err(|err| RemoteError::new(RemoteErrorKind::NetworkUnavailable, err))
}

pub fn webdav_send(
    client: &Client,
    method: Method,
    url: &str,
    username: &str,
    password: &str,
    if_match: Option<&str>,
    body: Option<Vec<u8>>,
) -> RemoteResult<Response> {
    let mut request = client
        .request(method, url)
        .basic_auth(username, Some(password));
    if let Some(etag) = if_match {
        request = request.header(reqwest::header::IF_MATCH, etag);
    }
    if let Some(bytes) = body {
        request = request.body(bytes);
    }
    request
        .send()
        .map_err(|error| RemoteError::new(classify_reqwest_error(&error), error.to_string()))
}

pub fn webdav_exists(
    client: &Client,
    url: &str,
    username: &str,
    password: &str,
) -> RemoteResult<bool> {
    let response = webdav_send(client, Method::HEAD, url, username, password, None, None)?;
    match response.status() {
        StatusCode::OK | StatusCode::NO_CONTENT | StatusCode::MULTI_STATUS => Ok(true),
        StatusCode::NOT_FOUND => Ok(false),
        _ => Err(error_from_response(&response, "WebDAV HEAD")),
    }
}

pub fn webdav_get_etag(
    client: &Client,
    url: &str,
    username: &str,
    password: &str,
) -> RemoteResult<Option<String>> {
    let response = webdav_send(client, Method::HEAD, url, username, password, None, None)?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(error_from_response(&response, "WebDAV HEAD"));
    }
    Ok(extract_etag_header(&response))
}

pub fn ensure_webdav_collection_chain(
    client: &Client,
    server_url: &str,
    target_url: &str,
    username: &str,
    password: &str,
) -> RemoteResult<()> {
    let server = Url::parse(server_url).map_err(|error| {
        RemoteError::new(
            RemoteErrorKind::PermissionDenied,
            format!("invalid server URL {server_url}: {error}"),
        )
    })?;
    let target = Url::parse(target_url).map_err(|error| {
        RemoteError::new(
            RemoteErrorKind::PermissionDenied,
            format!("invalid target URL {target_url}: {error}"),
        )
    })?;

    let base_segments = server
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut target_segments = target
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if !target.path().ends_with('/') && !target_segments.is_empty() {
        target_segments.pop();
    }

    let mkcol = Method::from_str("MKCOL").map_err(|error| {
        RemoteError::new(
            RemoteErrorKind::NetworkUnavailable,
            format!("failed to construct MKCOL method: {error}"),
        )
    })?;
    let mut current = server.clone();
    for segment in target_segments.into_iter().skip(base_segments.len()) {
        let next_path = format!("{}{}/", current.path(), segment);
        current.set_path(&next_path);
        let url_str = current.to_string();
        if webdav_exists(client, &url_str, username, password)? {
            continue;
        }
        let response = webdav_send(
            client,
            mkcol.clone(),
            &url_str,
            username,
            password,
            None,
            None,
        )?;
        match response.status() {
            StatusCode::CREATED
            | StatusCode::OK
            | StatusCode::NO_CONTENT
            | StatusCode::METHOD_NOT_ALLOWED => {}
            _ => return Err(error_from_response(&response, "WebDAV MKCOL")),
        }
    }

    Ok(())
}

pub fn webdav_conditional_put(
    client: &Client,
    url: &str,
    bytes: Vec<u8>,
    username: &str,
    password: &str,
    expected_revision: Option<&str>,
) -> RemoteResult<RemoteObjectMetadata> {
    let written_len = bytes.len() as u64;
    let mut request = client
        .request(Method::PUT, url)
        .basic_auth(username, Some(password))
        .body(bytes);

    match expected_revision {
        Some(rev) => {
            request = request.header(reqwest::header::IF_MATCH, rev);
        }
        None => {
            request = request.header(reqwest::header::IF_NONE_MATCH, "*");
        }
    }

    let response = request.send().map_err(|err| {
        RemoteError::new(
            classify_reqwest_error(&err),
            format!("WebDAV conditional PUT request failed: {err}"),
        )
    })?;

    let status = response.status();
    if status == StatusCode::PRECONDITION_FAILED
        || status == StatusCode::CONFLICT
        || (expected_revision.is_some() && status == StatusCode::NOT_FOUND)
    {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteConflict,
            format!("WebDAV conditional PUT precondition failed with HTTP {status}"),
        ));
    }
    if !status.is_success() {
        return Err(error_from_response(&response, "WebDAV conditional PUT"));
    }

    let etag = extract_etag_header(&response);
    let size = content_length_header(&response)
        .filter(|&n| n > 0)
        .or(Some(written_len));
    Ok(RemoteObjectMetadata {
        size_bytes: size,
        revision: etag,
    })
}

pub fn webdav_stat(
    client: &Client,
    url: &str,
    username: &str,
    password: &str,
) -> RemoteResult<Option<RemoteObjectMetadata>> {
    let response = webdav_send(client, Method::HEAD, url, username, password, None, None)?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(error_from_response(&response, "WebDAV stat HEAD"));
    }
    Ok(Some(RemoteObjectMetadata {
        size_bytes: content_length_header(&response),
        revision: extract_etag_header(&response),
    }))
}

fn extract_etag_header(response: &Response) -> Option<String> {
    response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned)
}

fn content_length_header(response: &Response) -> Option<u64> {
    response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
}

fn error_from_response(response: &Response, context: &str) -> RemoteError {
    let status = response.status();
    let kind = classify_status(status);
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_retry_after);
    remote_error_with_retry_after(
        kind,
        format!("{context} failed with HTTP {status}"),
        retry_after,
    )
}

fn content_range_window_len(header: &str) -> RemoteResult<u64> {
    let rest = header.strip_prefix("bytes").unwrap_or(header).trim();
    let (range_part, _) = rest.split_once('/').unwrap_or((rest, ""));
    let (start_str, end_str) = range_part.split_once('-').ok_or_else(|| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!("malformed Content-Range header: {header}"),
        )
    })?;
    let start: u64 = start_str.parse().map_err(|_| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!("malformed Content-Range start: {header}"),
        )
    })?;
    let end: u64 = end_str.parse().map_err(|_| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!("malformed Content-Range end: {header}"),
        )
    })?;
    if end < start {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!("invalid Content-Range bounds: {header}"),
        ));
    }
    Ok(end - start + 1)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PropfindField {
    Href,
    GetContentLength,
    GetEtag,
    GetLastModified,
}

#[derive(Default)]
struct PropfindEntry {
    href: String,
    is_collection: bool,
    size_bytes: Option<u64>,
    etag: Option<String>,
    modified_at_ms: Option<i64>,
}

pub fn parse_propfind_xml(
    xml: &str,
    root_url: &str,
    prefix: &str,
) -> RemoteResult<Vec<ListedObject>> {
    let root_path = Url::parse(root_url)
        .map(|u| u.path().to_string())
        .unwrap_or_else(|_| "/".to_string());

    let mut reader = Reader::from_str(xml);
    let mut entries = Vec::new();
    let mut current_entry: Option<PropfindEntry> = None;
    let mut current_field: Option<PropfindField> = None;
    let mut in_resourcetype = false;
    let mut text_buf = String::new();
    // quick-xml reports Eof without error when the document ends between
    // tags, so depth is tracked to reject truncated PROPFIND bodies.
    let mut open_elements: Vec<String> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                open_elements.push(e.name().as_ref().to_owned());
                let tag = xml_local_name(e.name().into_inner());
                match tag {
                    "response" => {
                        current_entry = Some(PropfindEntry::default());
                        current_field = None;
                        in_resourcetype = false;
                    }
                    "resourcetype" if current_entry.is_some() => {
                        in_resourcetype = true;
                    }
                    "collection" if in_resourcetype => {
                        if let Some(entry) = &mut current_entry {
                            entry.is_collection = true;
                        }
                    }
                    "href" if current_entry.is_some() => {
                        current_field = Some(PropfindField::Href);
                        text_buf.clear();
                    }
                    "getcontentlength" if current_entry.is_some() => {
                        current_field = Some(PropfindField::GetContentLength);
                        text_buf.clear();
                    }
                    "getetag" if current_entry.is_some() => {
                        current_field = Some(PropfindField::GetEtag);
                        text_buf.clear();
                    }
                    "getlastmodified" if current_entry.is_some() => {
                        current_field = Some(PropfindField::GetLastModified);
                        text_buf.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let tag = xml_local_name(e.name().into_inner());
                if tag == "collection" && in_resourcetype {
                    if let Some(entry) = &mut current_entry {
                        entry.is_collection = true;
                    }
                }
            }
            Ok(Event::Text(e)) => {
                if current_field.is_some() {
                    text_buf.push_str(&e.into_inner());
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if current_field.is_some() {
                    let entity = e.into_inner();
                    match entity.as_ref() {
                        "quot" => text_buf.push('"'),
                        "amp" => text_buf.push('&'),
                        "lt" => text_buf.push('<'),
                        "gt" => text_buf.push('>'),
                        "apos" => text_buf.push('\''),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) => {
                open_elements.pop();
                let tag = xml_local_name(e.name().into_inner());
                match tag {
                    "href" if current_field == Some(PropfindField::Href) => {
                        if let Some(entry) = &mut current_entry {
                            if entry.href.is_empty() {
                                entry.href = text_buf.trim().to_string();
                            }
                        }
                        current_field = None;
                    }
                    "getcontentlength"
                        if current_field == Some(PropfindField::GetContentLength) =>
                    {
                        if let Some(entry) = &mut current_entry {
                            entry.size_bytes = text_buf.trim().parse::<u64>().ok();
                        }
                        current_field = None;
                    }
                    "getetag" if current_field == Some(PropfindField::GetEtag) => {
                        if let Some(entry) = &mut current_entry {
                            let trimmed = text_buf.trim();
                            if !trimmed.is_empty() {
                                entry.etag = Some(trimmed.to_string());
                            }
                        }
                        current_field = None;
                    }
                    "getlastmodified" if current_field == Some(PropfindField::GetLastModified) => {
                        if let Some(entry) = &mut current_entry {
                            entry.modified_at_ms = parse_http_date_ms(text_buf.trim());
                        }
                        current_field = None;
                    }
                    "resourcetype" => {
                        in_resourcetype = false;
                    }
                    "response" => {
                        if let Some(entry) = current_entry.take() {
                            entries.push(entry);
                        }
                        current_field = None;
                        in_resourcetype = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => {
                if !open_elements.is_empty() {
                    return Err(RemoteError::new(
                        RemoteErrorKind::RemoteIntegrityFailed,
                        "truncated WebDAV PROPFIND XML: unclosed elements at end of document",
                    ));
                }
                break;
            }
            Err(err) => {
                return Err(RemoteError::new(
                    RemoteErrorKind::RemoteIntegrityFailed,
                    format!("failed to parse WebDAV PROPFIND XML: {err}"),
                ));
            }
            _ => {}
        }
    }

    let mut objects = Vec::new();
    for entry in entries {
        if entry.is_collection || entry.href.ends_with('/') {
            continue;
        }
        let Some(rel_path) = href_to_relative_path(&entry.href, &root_path) else {
            continue;
        };
        if validate_object_path(&rel_path).is_err() || !is_under_prefix(&rel_path, prefix) {
            continue;
        }
        objects.push(ListedObject {
            path: rel_path,
            metadata: ObjectMetadata {
                size_bytes: entry.size_bytes.unwrap_or(0),
                revision: entry.etag,
            },
            modified_at_ms: entry.modified_at_ms,
        });
    }
    objects.sort_by(|a, b| a.path.cmp(&b.path));
    objects.dedup_by(|a, b| a.path == b.path);
    Ok(objects)
}

fn parse_http_date_ms(value: &str) -> Option<i64> {
    let modified = httpdate::parse_http_date(value).ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(since_epoch.as_millis()).ok()
}

fn xml_local_name(raw: &str) -> &str {
    raw.rsplit_once(':').map_or(raw, |(_, local)| local)
}

fn href_to_relative_path(href: &str, root_path: &str) -> Option<String> {
    let href_path = if href.starts_with("http://") || href.starts_with("https://") {
        Url::parse(href).ok()?.path().to_string()
    } else {
        href.to_string()
    };

    let normalized_root = if root_path.ends_with('/') {
        root_path.to_string()
    } else {
        format!("{root_path}/")
    };

    let rel = href_path
        .strip_prefix(&normalized_root)
        .or_else(|| {
            href_path
                .trim_start_matches('/')
                .strip_prefix(normalized_root.trim_start_matches('/'))
        })?
        .trim_matches('/');

    if rel.is_empty() {
        None
    } else {
        Some(rel.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::MANIFEST_PATH;
    use crate::testing::{Fault, TestWebDavServer};

    crate::storage_conformance_suite!(TestWebDavServer::start());

    #[test]
    fn capabilities_can_disable_conditional_replace_and_range_reads() {
        let server = TestWebDavServer::start();
        let read_only = server
            .storage()
            .clone()
            .with_capabilities(StorageCapabilities {
                conditional_replace: false,
                range_read: false,
            });
        read_only
            .write_new("media/ab/one.flac", b"payload")
            .expect("write_new still works");

        let mut sink = Vec::new();
        let range_err = read_only
            .read_range("media/ab/one.flac", 0, 4, &mut sink)
            .expect_err("range read disabled");
        assert_eq!(
            range_err.kind,
            RemoteErrorKind::ProviderCapabilityUnavailable
        );

        let replace_err = read_only
            .conditional_replace("media/ab/one.flac", b"updated", "\"webdav-etag-1\"")
            .expect_err("conditional replace disabled");
        assert_eq!(
            replace_err.kind,
            RemoteErrorKind::ProviderCapabilityUnavailable
        );
    }

    #[test]
    fn scripted_faults_only_fire_on_their_own_path() {
        let server = TestWebDavServer::start();
        let storage = server.storage();
        storage
            .write_new("media/ab/one.flac", b"aaaa")
            .expect("one");
        storage
            .write_new("media/cd/two.flac", b"bbbb")
            .expect("two");

        server.script(Fault::InterruptTransfer {
            path: "media/ab/one.flac".into(),
            after_bytes: 1,
        });

        let mut sink = Vec::new();
        storage
            .read("media/cd/two.flac", &mut sink)
            .expect("other path unaffected");
        assert_eq!(sink, b"bbbb");

        sink.clear();
        let err = storage
            .read("media/ab/one.flac", &mut sink)
            .expect_err("matching path fails");
        assert_eq!(err.kind, RemoteErrorKind::NetworkUnavailable);
        assert_eq!(sink, b"a");
    }

    #[test]
    fn a_partial_write_waits_for_a_write_whose_precondition_holds() {
        let server = TestWebDavServer::start();
        let storage = server.storage();
        let base = storage
            .write_new(MANIFEST_PATH, b"generation 1")
            .expect("seed");
        server.script(Fault::PartialWrite {
            path: MANIFEST_PATH.into(),
            stored_bytes: 4,
        });

        let conflict = storage
            .conditional_replace(MANIFEST_PATH, b"wrong base", "stale-revision")
            .expect_err("stale precondition fails before partial write");
        assert_eq!(conflict.kind, RemoteErrorKind::RemoteConflict);
        let mut current = Vec::new();
        storage.read(MANIFEST_PATH, &mut current).expect("read");
        assert_eq!(current, b"generation 1");

        let partial = storage
            .conditional_replace(
                MANIFEST_PATH,
                b"generation 2",
                base.revision.as_deref().expect("revision"),
            )
            .expect_err("matching precondition triggers partial write");
        assert_eq!(partial.kind, RemoteErrorKind::NetworkUnavailable);
        current.clear();
        storage.read(MANIFEST_PATH, &mut current).expect("read");
        assert_eq!(current, b"gene");
    }

    #[test]
    fn server_and_root_urls_are_normalized_with_trailing_slashes() {
        let storage =
            WebDavStorage::new("https://dav.example.com/root", "u", "p").expect("construct");
        assert_eq!(storage.root_url(), "https://dav.example.com/root/");
        assert_eq!(storage.server_url(), "https://dav.example.com/root/");

        let split = WebDavStorage::with_server_and_root(
            "https://dav.example.com",
            "https://dav.example.com/libraries/main",
            "u",
            "p",
        )
        .expect("construct");
        assert_eq!(split.server_url(), "https://dav.example.com/");
        assert_eq!(split.root_url(), "https://dav.example.com/libraries/main/");

        assert!(WebDavStorage::new("not a url", "u", "p").is_err());
    }

    #[test]
    fn root_path_defaults_to_a_slugified_display_name() {
        assert_eq!(
            normalize_webdav_root_path(None, "My Library!"),
            "/My-Library"
        );
        assert_eq!(
            normalize_webdav_root_path(Some("  "), "My Library"),
            "/My-Library"
        );
        assert_eq!(
            normalize_webdav_root_path(Some("/custom/root/"), "ignored"),
            "/custom/root"
        );
        assert_eq!(
            normalize_webdav_root_path(Some("/"), "ignored"),
            "/OpenKara"
        );
        assert_eq!(normalize_webdav_root_path(None, "!!!"), "/OpenKara");
    }

    #[test]
    fn join_url_appends_relative_segments_within_the_root() {
        let joined = join_url("https://dav.example.com/root/", "media/ab/song.flac").expect("join");
        assert_eq!(joined, "https://dav.example.com/root/media/ab/song.flac");
        let with_leading = join_url("https://dav.example.com/root/", "/media/a").expect("join");
        assert_eq!(with_leading, "https://dav.example.com/root/media/a");
        assert!(join_url("not a url", "a").is_err());
    }

    #[test]
    fn object_urls_reject_paths_outside_the_repository_root() {
        let storage =
            WebDavStorage::new("https://dav.example.com/root", "u", "p").expect("construct");
        for bad in ["../escape", "media//a", "/absolute", ""] {
            assert!(storage.object_url(bad).is_err(), "{bad} must be rejected");
        }
    }

    #[test]
    fn remote_path_display_drops_scheme_and_credentials() {
        assert_eq!(
            remote_path_display_from_url("https://user:pw@dav.example.com/root/"),
            "dav.example.com/root"
        );
        assert_eq!(remote_path_display_from_url("not a url"), "not a url");
    }

    #[test]
    fn parse_propfind_xml_handles_escaped_etags_and_rejects_malformed_xml() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/OpenKara/media/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
      </D:prop>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/OpenKara/media/ab/song.flac</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getcontentlength>42</D:getcontentlength>
        <D:getetag>&quot;etag-42&quot;</D:getetag>
        <D:getlastmodified>Tue, 14 Nov 2023 22:13:20 GMT</D:getlastmodified>
      </D:prop>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

        let listed =
            parse_propfind_xml(xml, "https://dav.example.com/OpenKara/", "media").expect("parse");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].path, "media/ab/song.flac");
        assert_eq!(listed[0].metadata.size_bytes, 42);
        assert_eq!(listed[0].metadata.revision.as_deref(), Some("\"etag-42\""));
        assert_eq!(listed[0].modified_at_ms, Some(1_700_000_000_000));

        let err = parse_propfind_xml("<D:multistatus><unclosed>", "https://dav.example.com/", "")
            .expect_err("malformed XML");
        assert_eq!(err.kind, RemoteErrorKind::RemoteIntegrityFailed);
    }
}
