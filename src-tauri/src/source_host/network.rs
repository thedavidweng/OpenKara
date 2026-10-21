use super::types::*;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, LOCATION},
    Url,
};
use std::{
    collections::BTreeMap,
    io::Write,
    net::{IpAddr, SocketAddr},
    path::Path,
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
fn invalid() -> HostError {
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
    let mut url = Url::parse(url).map_err(|_| invalid())?;
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
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .resolve_to_addrs(&hostname, &addresses)
            .build()
            .map_err(|_| HostError::new("network", "Cannot prepare source download."))?;
        let response = tokio::select! {
            _=cancellation.cancelled()=>return Err(HostError::new("cancelled","Source download cancelled.")),
            response=client.get(url.clone()).headers(values.clone()).send()=>response.map_err(|_|HostError::new("network","Source download failed. Check the network, then retry."))?,
        };
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
                values.remove(reqwest::header::COOKIE);
            }
            url = next;
            continue;
        }
        return Ok(response);
    }
    Err(invalid())
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
            return Err(HostError::new(
                "invalid_resource",
                "Source response exceeds the permitted size.",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
pub(crate) async fn download(
    mut response: reqwest::Response,
    path: &Path,
    max: u64,
    cancel: &Cancellation,
    mut progress: impl FnMut(u64),
) -> HostResult<(u64, String)> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::create(path)?;
    let mut hash = Sha256::new();
    let mut size = 0;
    let result=async {
        loop {
            let chunk=tokio::select! { _=cancel.cancelled()=>return Err(HostError::new("cancelled","Source download cancelled. Retry when ready.")), chunk=response.chunk()=>chunk.map_err(|_|HostError::new("network","Source download was interrupted. Retry the download."))? };
            let Some(chunk)=chunk else { break };
            size+=chunk.len() as u64;
            if size>max { return Err(HostError::new("invalid_resource","Downloaded bytes exceed the declared size.")); }
            file.write_all(&chunk)?; hash.update(&chunk); progress(size);
        }
        cancel.check()?; file.sync_all()?;
        Ok((size,super::digest_hex(hash.finalize())))
    }.await;
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}
