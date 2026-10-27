use super::types::*;
use reqwest::{
    cookie::{CookieStore, Jar},
    header::{
        HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE, COOKIE, LOCATION,
        SET_COOKIE,
    },
    Method, Url,
};
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    time::Duration,
};

pub(crate) fn origin(value: &str) -> HostResult<String> {
    let url = Url::parse(value).map_err(|_| invalid())?;
    let authority = url
        .as_str()
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default())
        .unwrap_or_default();
    if url.scheme() != "https" || url.host_str().is_none() || authority.contains('@') {
        return Err(invalid());
    }
    Ok(url.origin().ascii_serialization())
}
pub(crate) fn invalid() -> HostError {
    HostError::new(
        "invalid_resource",
        "Source resource must use an approved public HTTPS origin and permitted headers.",
    )
}
pub(crate) fn check_expiry(descriptor: &ResourceDescriptor) -> HostResult<()> {
    if let Some(expiry) = &descriptor.expires_at {
        let expiry =
            time::OffsetDateTime::parse(expiry, &time::format_description::well_known::Rfc3339)
                .map_err(|_| invalid())?;
        if expiry <= time::OffsetDateTime::now_utc() {
            return Err(HostError::new(
                "media_expired",
                "Source resource has expired. Resolve a fresh descriptor.",
            ));
        }
    }
    Ok(())
}
fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
                || a == 0
                || a >= 240
                || (a == 100 && (64..=127).contains(&b))
                || (a == 192 && b == 0 && (c == 0 || c == 2))
                || (a == 198 && (b == 18 || b == 19 || b == 51))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public(IpAddr::V4(v4));
            }
            let segments = ip.segments();
            (segments[0] & 0xe000) == 0x2000
                && !(segments[0] == 0x2001 && (segments[1] == 0xdb8 || segments[1] < 0x0200))
        }
    }
}
fn headers(values: &BTreeMap<String, String>) -> HostResult<HeaderMap> {
    let mut result = HeaderMap::new();
    for (name, value) in values {
        if !["referer", "user-agent", "cookie"].contains(&name.to_ascii_lowercase().as_str())
            || value.contains(['\r', '\n'])
        {
            return Err(invalid());
        }
        if name.eq_ignore_ascii_case("referer") {
            origin(value)?;
        }
        let key = HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid())?;
        if result.contains_key(&key) {
            return Err(invalid());
        }
        result.insert(key, HeaderValue::from_str(value).map_err(|_| invalid())?);
    }
    Ok(result)
}
pub(crate) async fn response(
    url: &str,
    values: &BTreeMap<String, String>,
    origins: Option<&[String]>,
    test_distribution: bool,
    cancellation: &Cancellation,
) -> HostResult<reqwest::Response> {
    response_range(url, values, origins, test_distribution, cancellation, None)
        .await?
        .error_for_status()
        .map_err(|_| HostError::new("network", "Source download is unavailable. Retry later."))
}
pub(crate) async fn response_range(
    url: &str,
    values: &BTreeMap<String, String>,
    origins: Option<&[String]>,
    test_distribution: bool,
    cancellation: &Cancellation,
    range: Option<(u64, Option<u64>)>,
) -> HostResult<reqwest::Response> {
    let mut values = headers(values)?;
    if let Some((start, end)) = range {
        if end.is_some_and(|end| end < start) {
            return Err(invalid());
        }
        let range = format!(
            "bytes={start}-{}",
            end.map(|end| end.to_string()).unwrap_or_default()
        );
        values.insert(
            reqwest::header::RANGE,
            HeaderValue::from_str(&range).map_err(|_| invalid())?,
        );
    }
    let outgoing = Outgoing {
        method: Method::GET,
        headers: values,
        body: None,
        cookies: None,
        timeout: Duration::from_secs(120),
        #[cfg(test)]
        upstreams: None,
    };
    send(url, outgoing, origins, test_distribution, cancellation)
        .await
        .map(|(response, _)| response)
}

struct Outgoing<'a> {
    method: Method,
    headers: HeaderMap,
    body: Option<Vec<u8>>,
    cookies: Option<&'a Jar>,
    timeout: Duration,
    #[cfg(test)]
    upstreams: Option<&'a BTreeMap<String, String>>,
}

/// Sends one request and follows redirects by hand, so every hop passes the
/// HTTPS, origin and public-address checks before it connects.
async fn send(
    url: &str,
    mut outgoing: Outgoing<'_>,
    origins: Option<&[String]>,
    test_distribution: bool,
    cancellation: &Cancellation,
) -> HostResult<(reqwest::Response, Url)> {
    let mut url = Url::parse(url).map_err(|_| invalid())?;
    for hop in 0..=5 {
        cancellation.check()?;
        let hostname = url.host_str().ok_or_else(invalid)?.to_string();
        let test_loopback = cfg!(test)
            && test_distribution
            && url.scheme() == "http"
            && matches!(hostname.as_str(), "127.0.0.1" | "localhost" | "[::1]");
        if !test_loopback {
            origin(url.as_str())?;
        }
        if let Some(origins) = origins {
            if !origins
                .iter()
                .any(|allowed| allowed == &url.origin().ascii_serialization())
            {
                return Err(invalid());
            }
        }
        #[cfg(test)]
        let upstream = outgoing
            .upstreams
            .and_then(|upstreams| upstreams.get(&url.origin().ascii_serialization()));
        #[cfg(not(test))]
        let upstream: Option<&String> = None;
        let builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(outgoing.timeout);
        let (builder, target) = if let Some(upstream) = upstream {
            let query = url
                .query()
                .map(|query| format!("?{query}"))
                .unwrap_or_default();
            let target =
                Url::parse(&format!("{upstream}{}{query}", url.path())).map_err(|_| invalid())?;
            (builder, target)
        } else {
            if let Ok(ip) = hostname.trim_matches(['[', ']']).parse::<IpAddr>() {
                if !test_loopback && !public(ip) {
                    return Err(invalid());
                }
            }
            let addresses: Vec<SocketAddr> = tokio::select! {
                _=cancellation.cancelled()=>return Err(HostError::new("cancelled","Source download cancelled.")),
                resolved=tokio::time::timeout(Duration::from_secs(15),tokio::net::lookup_host((hostname.trim_matches(['[',']']),url.port_or_known_default().ok_or_else(invalid)?)))=>resolved.map_err(|_|HostError::new("network","Source DNS lookup timed out. Check the network, then retry."))?.map_err(|_|HostError::new("network","Cannot resolve source download. Check the network, then retry."))?.collect(),
            };
            if addresses.is_empty()
                || (!test_loopback && addresses.iter().any(|address| !public(address.ip())))
            {
                return Err(invalid());
            }
            (builder.resolve_to_addrs(&hostname, &addresses), url.clone())
        };
        let client = builder
            .build()
            .map_err(|_| HostError::new("network", "Cannot prepare source download."))?;
        let mut values = outgoing.headers.clone();
        if let Some(stored) = outgoing.cookies.and_then(|jar| jar.cookies(&url)) {
            let merged = match values.get(COOKIE).and_then(|value| value.to_str().ok()) {
                Some(explicit) => HeaderValue::from_str(&format!(
                    "{explicit}; {}",
                    stored.to_str().map_err(|_| invalid())?
                ))
                .map_err(|_| invalid())?,
                None => stored,
            };
            values.insert(COOKIE, merged);
        }
        let mut request = client
            .request(outgoing.method.clone(), target)
            .headers(values);
        if let Some(body) = &outgoing.body {
            request = request.body(body.clone());
        }
        let response = tokio::select! {
            _=cancellation.cancelled()=>return Err(HostError::new("cancelled","Source download cancelled.")),
            response=request.send()=>response.map_err(|_|HostError::new("network","Source download failed. Check the network, then retry."))?,
        };
        if let Some(jar) = outgoing.cookies {
            jar.set_cookies(&mut response.headers().get_all(SET_COOKIE).iter(), &url);
        }
        if response.status().is_redirection() {
            if hop == 5 {
                return Err(HostError::new(
                    "network",
                    "Source download exceeded the redirect limit.",
                ));
            }
            let next = url
                .join(
                    response
                        .headers()
                        .get(LOCATION)
                        .ok_or_else(invalid)?
                        .to_str()
                        .map_err(|_| invalid())?,
                )
                .map_err(|_| invalid())?;
            if next.origin() != url.origin() {
                outgoing.headers.remove(COOKIE);
                outgoing.headers.remove(AUTHORIZATION);
            }
            let status = response.status().as_u16();
            if (status == 303 && outgoing.method != Method::HEAD)
                || (matches!(status, 301 | 302) && outgoing.method == Method::POST)
            {
                outgoing.method = Method::GET;
                outgoing.body = None;
                outgoing.headers.remove(CONTENT_TYPE);
            }
            url = next;
            continue;
        }
        return Ok((response, url));
    }
    Err(invalid())
}

pub(crate) const MAX_SOURCE_REQUEST_BODY: usize = 1024 * 1024;
pub(crate) const MAX_SOURCE_RESPONSE: u64 = 8 * 1024 * 1024;

/// What one source may reach: its declared origins and its own cookie jar.
pub(crate) struct SourceNetwork {
    pub(crate) origins: Vec<String>,
    pub(crate) cookies: Jar,
    pub(crate) timeout: Duration,
    /// Plain-HTTP servers that stand in for declared HTTPS origins in tests.
    #[cfg(test)]
    pub(crate) upstreams: BTreeMap<String, String>,
}
impl SourceNetwork {
    pub(crate) fn without_cookies(&self) -> Self {
        Self {
            origins: self.origins.clone(),
            cookies: Jar::default(),
            timeout: self.timeout,
            #[cfg(test)]
            upstreams: self.upstreams.clone(),
        }
    }
}
pub(crate) struct SourceRequest {
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: Option<Vec<u8>>,
}
pub(crate) struct SourceResponse {
    pub(crate) status: u16,
    pub(crate) url: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

/// Headers a source may not set: the host owns framing, the connection, the
/// target host and proxy credentials.
fn forbidden_request_header(name: &str) -> bool {
    matches!(
        name,
        "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "keep-alive"
            | "upgrade"
            | "te"
            | "trailer"
            | "expect"
    ) || name.starts_with("proxy-")
        || name.starts_with("sec-")
}
fn request_headers(values: &BTreeMap<String, String>) -> HostResult<HeaderMap> {
    let mut result = HeaderMap::new();
    for (name, value) in values {
        let lower = name.to_ascii_lowercase();
        if forbidden_request_header(&lower) || value.contains(['\r', '\n', '\0']) {
            return Err(invalid());
        }
        if matches!(lower.as_str(), "referer" | "origin") {
            origin(value)?;
        }
        let key = HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid())?;
        if result.contains_key(&key) {
            return Err(invalid());
        }
        result.insert(key, HeaderValue::from_str(value).map_err(|_| invalid())?);
    }
    Ok(result)
}

pub(crate) async fn source_request(
    network: &SourceNetwork,
    request: SourceRequest,
    cancellation: &Cancellation,
) -> HostResult<SourceResponse> {
    let method = match request.method.to_ascii_uppercase().as_str() {
        "GET" => Method::GET,
        "HEAD" => Method::HEAD,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "PATCH" => Method::PATCH,
        "DELETE" => Method::DELETE,
        _ => return Err(invalid()),
    };
    if request
        .body
        .as_ref()
        .is_some_and(|body| body.len() > MAX_SOURCE_REQUEST_BODY)
    {
        return Err(HostError::new(
            "invalid_resource",
            "Source request body exceeds the permitted size.",
        ));
    }
    let outgoing = Outgoing {
        method,
        headers: request_headers(&request.headers)?,
        body: request.body,
        cookies: Some(&network.cookies),
        timeout: network.timeout,
        #[cfg(test)]
        upstreams: Some(&network.upstreams),
    };
    let (response, url) = send(
        &request.url,
        outgoing,
        Some(&network.origins),
        false,
        cancellation,
    )
    .await?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_SOURCE_RESPONSE)
    {
        return Err(oversized());
    }
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_owned(), value.to_owned()))
        })
        .collect();
    let body = read_bounded(response, MAX_SOURCE_RESPONSE, cancellation).await?;
    Ok(SourceResponse {
        status,
        url: url.to_string(),
        headers,
        body,
    })
}
fn oversized() -> HostError {
    HostError::new(
        "invalid_resource",
        "Source response exceeds the permitted size.",
    )
}

pub struct MediaResponse {
    pub status: u16,
    pub content_length: Option<u64>,
    pub content_type: Option<String>,
    pub content_range: Option<String>,
    response: Option<reqwest::Response>,
    cancellation: Cancellation,
    #[cfg(test)]
    fixture: Option<std::io::Cursor<Vec<u8>>>,
}
impl MediaResponse {
    pub(crate) fn new(response: reqwest::Response, cancellation: Cancellation) -> Self {
        Self {
            status: response.status().as_u16(),
            content_length: response.content_length(),
            content_type: response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            content_range: response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            response: Some(response),
            cancellation,
            #[cfg(test)]
            fixture: None,
        }
    }
    pub async fn chunk(&mut self) -> HostResult<Option<Vec<u8>>> {
        self.cancellation.check()?;
        #[cfg(test)]
        if let Some(fixture) = &mut self.fixture {
            use std::io::Read;
            let mut chunk = vec![0u8; 64 * 1024];
            let count = fixture.read(&mut chunk)?;
            chunk.truncate(count);
            return Ok((count > 0).then_some(chunk));
        }
        let response = self.response.as_mut().expect("network response");
        tokio::select! {_=self.cancellation.cancelled()=>Err(HostError::new("cancelled","Source media request cancelled.")),chunk=response.chunk()=>chunk.map(|chunk|chunk.map(|bytes|bytes.to_vec())).map_err(|_|HostError::new("network","Source media transfer was interrupted. Retry the request."))}
    }
}
#[cfg(test)]
pub(crate) fn fixture_media(
    config: &HostConfig,
    descriptor: &ResourceDescriptor,
    origins: &[String],
    range: Option<(u64, Option<u64>)>,
    cancellation: Cancellation,
) -> Option<HostResult<MediaResponse>> {
    config
        .fixture_resources
        .get(&descriptor.url)
        .map(|(addresses, bytes)| {
            let approved = origin(&descriptor.url)?;
            if !origins.contains(&approved)
                || addresses.is_empty()
                || addresses.iter().any(|ip| !public(*ip))
            {
                return Err(invalid());
            }
            headers(&descriptor.headers)?;
            let size = bytes.len() as u64;
            let (status, start, end, content_range) = if let Some((start, end)) = range {
                let end = end
                    .unwrap_or(size.saturating_sub(1))
                    .min(size.saturating_sub(1));
                if start > end || start >= size {
                    return Err(invalid());
                }
                (
                    206,
                    start,
                    end + 1,
                    Some(format!("bytes {start}-{end}/{size}")),
                )
            } else {
                (
                    config
                        .fixture_statuses
                        .get(&descriptor.url)
                        .copied()
                        .unwrap_or(200),
                    0,
                    size,
                    None,
                )
            };
            Ok(MediaResponse {
                status,
                content_length: Some(end - start),
                content_type: None,
                content_range,
                response: None,
                cancellation,
                fixture: Some(std::io::Cursor::new(
                    bytes[start as usize..end as usize].to_vec(),
                )),
            })
        })
}
pub(crate) async fn read_bounded(
    mut response: reqwest::Response,
    max: u64,
    cancel: &Cancellation,
) -> HostResult<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::select! { _=cancel.cancelled()=>return Err(HostError::new("cancelled","Source download cancelled.")), chunk=response.chunk()=>chunk.map_err(|_|HostError::new("network","Source download was interrupted. Retry the download."))? };
        let Some(chunk) = chunk else { break };
        if bytes.len() as u64 + chunk.len() as u64 > max {
            return Err(oversized());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
