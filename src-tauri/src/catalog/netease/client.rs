use super::crypto::{md5_hex, weapi_encrypt};
use crate::catalog::types::{CatalogError, StreamingCredentials};
use rand::RngExt;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, COOKIE, USER_AGENT};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

const API_BASE: &str = "https://music.163.com";
const USER_AGENT_VALUE: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

const CHINA_IP_RANGES: &[(u8, u8, u8)] = &[
    (36, 56, 0),
    (42, 202, 0),
    (49, 64, 0),
    (58, 192, 0),
    (60, 160, 0),
    (111, 206, 0),
    (114, 80, 0),
    (123, 125, 0),
    (180, 149, 0),
    (202, 108, 0),
];

pub fn china_client_address() -> String {
    let mut rng = rand::rng();
    let (a, b, c) = CHINA_IP_RANGES[rng.random_range(0..CHINA_IP_RANGES.len())];
    let d = rng.random_range(1..255);
    format!("{a}.{b}.{c}.{d}")
}

pub fn attach_china_client_address(headers: &mut HeaderMap, address: &str) {
    if let Ok(value) = HeaderValue::from_str(address) {
        headers.insert("X-Real-IP", value.clone());
        headers.insert("X-Forwarded-For", value);
    }
}

#[derive(Debug, Clone)]
pub struct NeteaseHttpResponse {
    pub json: Value,
    pub cookies: HashMap<String, String>,
}

pub trait NeteaseHttp: Send + Sync {
    fn post_weapi(
        &self,
        path: &str,
        payload: Value,
        credentials: Option<&StreamingCredentials>,
    ) -> Result<NeteaseHttpResponse, CatalogError>;

    fn download(
        &self,
        url: &str,
        dest: &Path,
        credentials: Option<&StreamingCredentials>,
    ) -> Result<(), CatalogError>;

    fn last_china_address(&self) -> Option<String> {
        None
    }
}

pub struct LiveNeteaseHttp {
    client: Client,
    api_base: String,
    last_address: Mutex<Option<String>>,
}

impl LiveNeteaseHttp {
    pub fn new() -> Result<Self, CatalogError> {
        Self::with_api_base(API_BASE)
    }

    pub fn with_api_base(api_base: &str) -> Result<Self, CatalogError> {
        let client = Client::builder()
            .user_agent(USER_AGENT_VALUE)
            .build()
            .map_err(|error| CatalogError::Network(error.to_string()))?;
        Ok(Self {
            client,
            api_base: api_base.to_owned(),
            last_address: Mutex::new(None),
        })
    }

    fn headers(&self, credentials: Option<&StreamingCredentials>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let address = china_client_address();
        if let Ok(mut last) = self.last_address.lock() {
            *last = Some(address.clone());
        }
        attach_china_client_address(&mut headers, &address);
        headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
        if let Some(credentials) = credentials {
            let cookie = format!(
                "MUSIC_U={}; __csrf={}",
                credentials.music_u, credentials.csrf
            );
            if let Ok(value) = HeaderValue::from_str(&cookie) {
                headers.insert(COOKIE, value);
            }
        }
        headers
    }
}

impl NeteaseHttp for LiveNeteaseHttp {
    fn post_weapi(
        &self,
        path: &str,
        mut payload: Value,
        credentials: Option<&StreamingCredentials>,
    ) -> Result<NeteaseHttpResponse, CatalogError> {
        if let Some(credentials) = credentials {
            if let Some(object) = payload.as_object_mut() {
                object.insert("csrf_token".to_owned(), json!(credentials.csrf));
            }
        }
        let form = weapi_encrypt(&payload.to_string());
        let url = format!("{}{path}", self.api_base);
        let response = self
            .client
            .post(url)
            .headers(self.headers(credentials))
            .form(&[
                ("params", form.params.as_str()),
                ("encSecKey", form.enc_sec_key.as_str()),
            ])
            .send()
            .map_err(|error| CatalogError::Network(error.to_string()))?;

        if response.status().as_u16() == 301 {
            return Err(CatalogError::SessionExpired {
                source_id: "netease".to_owned(),
            });
        }

        let cookies = cookie_map(response.headers());
        let json = response
            .json::<Value>()
            .map_err(|error| CatalogError::Network(error.to_string()))?;
        if json.get("code").and_then(Value::as_i64) == Some(301) {
            return Err(CatalogError::SessionExpired {
                source_id: "netease".to_owned(),
            });
        }
        Ok(NeteaseHttpResponse { json, cookies })
    }

    fn download(
        &self,
        url: &str,
        dest: &Path,
        credentials: Option<&StreamingCredentials>,
    ) -> Result<(), CatalogError> {
        let bytes = self
            .client
            .get(url)
            .headers(self.headers(credentials))
            .send()
            .and_then(|response| response.bytes())
            .map_err(|error| CatalogError::Network(error.to_string()))?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| CatalogError::Internal(error.to_string()))?;
        }
        std::fs::write(dest, bytes).map_err(|error| CatalogError::Internal(error.to_string()))
    }

    fn last_china_address(&self) -> Option<String> {
        self.last_address
            .lock()
            .ok()
            .and_then(|value| value.clone())
    }
}

fn cookie_map(headers: &HeaderMap) -> HashMap<String, String> {
    let mut cookies = HashMap::new();
    for value in headers.get_all(reqwest::header::SET_COOKIE) {
        let Ok(text) = value.to_str() else {
            continue;
        };
        if let Some((pair, _)) = text.split_once(';') {
            if let Some((name, cookie_value)) = pair.split_once('=') {
                cookies.insert(name.trim().to_owned(), cookie_value.trim().to_owned());
            }
        }
    }
    cookies
}

pub fn hashed_password(password: &str) -> String {
    md5_hex(password)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn china_client_address_is_dotted_ipv4() {
        let address = china_client_address();
        let parts: Vec<_> = address.split('.').collect();
        assert_eq!(parts.len(), 4);
        assert!(CHINA_IP_RANGES
            .iter()
            .any(|(a, b, c)| format!("{a}.{b}.{c}") == parts[..3].join(".")));
    }

    #[test]
    fn attach_writes_real_ip_headers() {
        let mut headers = HeaderMap::new();
        attach_china_client_address(&mut headers, "114.80.0.12");
        assert_eq!(
            headers
                .get("X-Real-IP")
                .and_then(|value| value.to_str().ok()),
            Some("114.80.0.12")
        );
        assert_eq!(
            headers
                .get("X-Forwarded-For")
                .and_then(|value| value.to_str().ok()),
            Some("114.80.0.12")
        );
    }
}
