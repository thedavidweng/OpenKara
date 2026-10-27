use super::{
    crypto::{self, AesOptions, Direction},
    log::{Level, SourceLog},
    storage::Storage,
};
use crate::source_host::{
    network::{self, SourceNetwork, SourceRequest},
    runtime::unavailable,
    types::*,
};
use rquickjs::{
    function::{Async, Opt},
    ArrayBuffer, Ctx, Exception, Function, IntoJs, Object, Result, TypedArray, Value,
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

const MAX_IN_FLIGHT_REQUESTS: usize = 8;
const MAX_TIMERS: usize = 256;
const MAX_TIMER_DELAY_MS: f64 = 60_000.0;

/// What the host side prepares for one source before its engine thread starts.
pub(in crate::source_host) struct HostSetup {
    pub(super) source_id: String,
    pub(super) owner_binding: String,
    pub(super) storage_root: PathBuf,
    pub(super) network: SourceNetwork,
    pub(super) runtime: tokio::runtime::Handle,
    pub(super) dispatch: tracing::Dispatch,
}

#[derive(Default)]
struct Timers {
    next: u32,
    live: BTreeSet<u32>,
}

/// The host API of one source, owned by its engine thread.
pub(super) struct Host {
    network: RefCell<Arc<SourceNetwork>>,
    runtime: tokio::runtime::Handle,
    shutdown: Cancellation,
    storage: RefCell<Storage>,
    log: RefCell<SourceLog>,
    timers: RefCell<Timers>,
    in_flight: Cell<usize>,
}
impl Host {
    pub(super) fn new(setup: HostSetup, shutdown: Cancellation) -> Rc<Self> {
        Rc::new(Self {
            storage: RefCell::new(Storage::open(
                &setup.storage_root,
                &setup.source_id,
                &setup.owner_binding,
            )),
            log: RefCell::new(SourceLog::new(setup.source_id, setup.dispatch)),
            network: RefCell::new(Arc::new(setup.network)),
            runtime: setup.runtime,
            shutdown,
            timers: RefCell::default(),
            in_flight: Cell::new(0),
        })
    }
    /// Notes the credential values a protocol call carries in or out, so
    /// logging can redact them.
    pub(super) fn observe_credentials(&self, message: &serde_json::Value) {
        self.log
            .borrow_mut()
            .remember_credentials(&message["credentials"]);
    }
    /// Signing out ends the account on this device, so the cookies that the
    /// host collected for the source go with it.
    pub(super) fn forget_cookies(&self) {
        let fresh = self.network.borrow().without_cookies();
        *self.network.borrow_mut() = Arc::new(fresh);
    }
    async fn request<'js>(&self, ctx: Ctx<'js>, options: Object<'js>) -> Result<Value<'js>> {
        let url: String = options.get("url")?;
        let method: Option<String> = options.get("method")?;
        let mut headers = BTreeMap::new();
        if let Some(values) = options.get::<_, Option<Object>>("headers")? {
            for entry in values.props::<String, String>() {
                let (name, value) = entry?;
                if matches!(
                    name.to_ascii_lowercase().as_str(),
                    "cookie" | "authorization"
                ) {
                    self.log.borrow_mut().remember(&value);
                }
                headers.insert(name, value);
            }
        }
        let body = match options.get::<_, Value>("body")? {
            value if value.is_undefined() || value.is_null() => None,
            value => Some(bytes(&ctx, &value)?),
        };
        let response_type = options
            .get::<_, Option<String>>("responseType")?
            .unwrap_or_else(|| "text".into());
        if !matches!(response_type.as_str(), "text" | "json" | "bytes") {
            return Err(Exception::throw_type(
                &ctx,
                "responseType must be text, json or bytes",
            ));
        }
        if self.in_flight.get() >= MAX_IN_FLIGHT_REQUESTS {
            return Err(throw(
                &ctx,
                HostError::new("busy", "Too many source requests are in flight."),
            ));
        }
        let request = SourceRequest {
            url,
            method: method.unwrap_or_else(|| "GET".into()),
            headers,
            body,
        };
        let network = self.network.borrow().clone();
        let cancellation = self.shutdown.clone();
        let task = self
            .runtime
            .spawn(async move { network::source_request(&network, request, &cancellation).await });
        let _abort = AbortOnDrop(task.abort_handle());
        self.in_flight.set(self.in_flight.get() + 1);
        let _slot = Slot(&self.in_flight);
        let response = match task.await {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => return Err(throw(&ctx, error)),
            Err(_) => return Err(throw(&ctx, unavailable())),
        };
        let object = Object::new(ctx.clone())?;
        object.set("status", response.status)?;
        object.set("url", response.url)?;
        let headers = Object::new(ctx.clone())?;
        let mut set_cookies = Vec::new();
        for (name, value) in response.headers {
            if name == "set-cookie" {
                if let Some((_, rest)) = value.split_once('=') {
                    self.log
                        .borrow_mut()
                        .remember(rest.split(';').next().unwrap_or_default());
                }
                set_cookies.push(value);
                continue;
            }
            let joined = match headers.get::<_, Option<String>>(name.as_str())? {
                Some(previous) => format!("{previous}, {value}"),
                None => value,
            };
            headers.set(name, joined)?;
        }
        object.set("headers", headers)?;
        object.set("setCookies", set_cookies)?;
        let body = match response_type.as_str() {
            "bytes" => TypedArray::<u8>::new(ctx.clone(), response.body)?.into_value(),
            "json" => ctx.json_parse(response.body)?,
            _ => String::from_utf8_lossy(&response.body)
                .into_owned()
                .into_js(&ctx)?,
        };
        object.set("body", body)?;
        Ok(object.into_value())
    }
}

struct AbortOnDrop(tokio::task::AbortHandle);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct Slot<'a>(&'a Cell<usize>);
impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get().saturating_sub(1));
    }
}

/// Rejects with an `Error` whose `code` is the host error code, so a source
/// that rethrows it reports the matching protocol error.
fn throw(ctx: &Ctx<'_>, error: HostError) -> rquickjs::Error {
    match Exception::from_message(ctx.clone(), &error.message) {
        Ok(exception) => match exception.as_object().set("code", error.code) {
            Ok(()) => ctx.throw(exception.into_value()),
            Err(error) => error,
        },
        Err(error) => error,
    }
}

/// Reads binary input: a `Uint8Array`, an `ArrayBuffer`, or a string as UTF-8.
fn bytes<'js>(ctx: &Ctx<'js>, value: &Value<'js>) -> Result<Vec<u8>> {
    if let Some(text) = value.as_string() {
        return Ok(text.to_string()?.into_bytes());
    }
    if let Ok(array) = TypedArray::<u8>::from_value(value.clone()) {
        // SAFETY: the bytes are copied at once while this thread holds the
        // runtime lock, so no script can detach or resize the buffer meanwhile.
        if let Some(bytes) = unsafe { array.as_bytes() } {
            return Ok(bytes.to_vec());
        }
    }
    if let Some(buffer) = ArrayBuffer::from_value(value.clone()) {
        // SAFETY: as above, the copy completes before any script runs again.
        if let Some(bytes) = unsafe { buffer.as_bytes() } {
            return Ok(bytes.to_vec());
        }
    }
    Err(Exception::throw_type(
        ctx,
        "expected a Uint8Array, an ArrayBuffer or a string",
    ))
}

fn output<'js>(
    ctx: &Ctx<'js>,
    result: crypto::CryptoResult<Vec<u8>>,
) -> Result<TypedArray<'js, u8>> {
    match result {
        Ok(bytes) => TypedArray::new(ctx.clone(), bytes),
        Err(message) => Err(Exception::throw_type(ctx, message)),
    }
}

fn aes_options<'js>(
    ctx: &Ctx<'js>,
    options: &Object<'js>,
) -> Result<(String, Vec<u8>, Option<Vec<u8>>, bool, Vec<u8>)> {
    let mode: String = options.get("mode")?;
    let key = bytes(ctx, &options.get("key")?)?;
    let iv = match options.get::<_, Value>("iv")? {
        value if value.is_undefined() || value.is_null() => None,
        value => Some(bytes(ctx, &value)?),
    };
    let padding = match options.get::<_, Option<String>>("padding")?.as_deref() {
        None | Some("pkcs7") => true,
        Some("none") => false,
        Some(_) => return Err(Exception::throw_type(ctx, "padding must be pkcs7 or none")),
    };
    let aad = match options.get::<_, Value>("aad")? {
        value if value.is_undefined() || value.is_null() => Vec::new(),
        value => bytes(ctx, &value)?,
    };
    Ok((mode, key, iv, padding, aad))
}

fn aes<'js>(
    ctx: Ctx<'js>,
    direction: Direction,
    options: Object<'js>,
    data: Value<'js>,
) -> Result<TypedArray<'js, u8>> {
    let (mode, key, iv, padding, aad) = aes_options(&ctx, &options)?;
    let data = bytes(&ctx, &data)?;
    let options = AesOptions {
        mode: &mode,
        key: &key,
        iv: iv.as_deref(),
        padding,
        aad: &aad,
    };
    output(&ctx, crypto::aes(direction, &options, &data))
}

fn rsa_public<'js>(
    ctx: Ctx<'js>,
    key: Object<'js>,
    data: Value<'js>,
) -> Result<TypedArray<'js, u8>> {
    let (modulus, exponent) = match key.get::<_, Option<String>>("pem")? {
        Some(pem) => match crypto::rsa_public_key_pem(&pem) {
            Ok(parts) => parts,
            Err(message) => return Err(Exception::throw_type(&ctx, message)),
        },
        None => (
            bytes(&ctx, &key.get("modulus")?)?,
            bytes(&ctx, &key.get("exponent")?)?,
        ),
    };
    let data = bytes(&ctx, &data)?;
    output(&ctx, crypto::rsa_public(&modulus, &exponent, &data))
}

fn log_record<'js>(
    ctx: &Ctx<'js>,
    host: &Host,
    level: Level,
    message: Value<'js>,
    fields: Opt<Value<'js>>,
) -> Result<()> {
    let message = match message.as_string() {
        Some(text) => text.to_string()?,
        None => ctx
            .json_stringify(message)?
            .map(|text| text.to_string())
            .transpose()?
            .unwrap_or_default(),
    };
    let fields = match fields.0 {
        Some(fields) if !fields.is_undefined() => ctx
            .json_stringify(fields)?
            .map(|text| text.to_string())
            .transpose()?
            .and_then(|json| serde_json::from_str(&json).ok()),
        _ => None,
    };
    host.log.borrow_mut().record(level, &message, fields);
    Ok(())
}

fn storage_result(
    ctx: &Ctx<'_>,
    result: std::result::Result<(), super::storage::StorageError>,
) -> Result<()> {
    result.map_err(|error| Exception::throw_message(ctx, error.message()))
}

/// Installs the global `openkara` host object and the timer functions. These
/// are the only capabilities a source gets.
pub(super) fn install<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> Result<()> {
    let api = Object::new(ctx.clone())?;

    let request_host = host.clone();
    api.set(
        "request",
        Function::new(
            ctx.clone(),
            Async(move |ctx: Ctx<'js>, options: Object<'js>| {
                let host = request_host.clone();
                async move { host.request(ctx, options).await }
            }),
        )?,
    )?;

    let crypto_api = Object::new(ctx.clone())?;
    crypto_api.set(
        "digest",
        Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, algorithm: String, data: Value<'js>| {
                let data = bytes(&ctx, &data)?;
                output(&ctx, crypto::digest(&algorithm, &data))
            },
        )?,
    )?;
    crypto_api.set(
        "hmac",
        Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, algorithm: String, key: Value<'js>, data: Value<'js>| {
                let key = bytes(&ctx, &key)?;
                let data = bytes(&ctx, &data)?;
                output(&ctx, crypto::hmac(&algorithm, &key, &data))
            },
        )?,
    )?;
    crypto_api.set(
        "aesEncrypt",
        Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, options: Object<'js>, data: Value<'js>| {
                aes(ctx, Direction::Encrypt, options, data)
            },
        )?,
    )?;
    crypto_api.set(
        "aesDecrypt",
        Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, options: Object<'js>, data: Value<'js>| {
                aes(ctx, Direction::Decrypt, options, data)
            },
        )?,
    )?;
    crypto_api.set("rsaPublic", Function::new(ctx.clone(), rsa_public)?)?;
    crypto_api.set(
        "randomBytes",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, length: u32| {
            output(&ctx, crypto::random_bytes(length as usize))
        })?,
    )?;
    api.set("crypto", crypto_api)?;

    let encoding = Object::new(ctx.clone())?;
    encoding.set(
        "base64Encode",
        Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, data: Value<'js>, options: Opt<Object<'js>>| -> Result<String> {
                let data = bytes(&ctx, &data)?;
                let (url_safe, padding) = match options.0 {
                    Some(options) => (
                        options.get::<_, Option<bool>>("urlSafe")?.unwrap_or(false),
                        options.get::<_, Option<bool>>("padding")?.unwrap_or(true),
                    ),
                    None => (false, true),
                };
                Ok(crypto::base64_encode(&data, url_safe, padding))
            },
        )?,
    )?;
    encoding.set(
        "base64Decode",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, text: String| {
            output(&ctx, crypto::base64_decode(&text))
        })?,
    )?;
    encoding.set(
        "hexEncode",
        Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, data: Value<'js>| -> Result<String> {
                Ok(crypto::hex_encode(&bytes(&ctx, &data)?))
            },
        )?,
    )?;
    encoding.set(
        "hexDecode",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, text: String| {
            output(&ctx, crypto::hex_decode(&text))
        })?,
    )?;
    encoding.set(
        "utf8Encode",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, text: String| {
            TypedArray::<u8>::new(ctx, text.into_bytes())
        })?,
    )?;
    encoding.set(
        "utf8Decode",
        Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, data: Value<'js>| -> Result<String> {
                Ok(String::from_utf8_lossy(&bytes(&ctx, &data)?).into_owned())
            },
        )?,
    )?;
    api.set("encoding", encoding)?;

    let storage = Object::new(ctx.clone())?;
    let storage_host = host.clone();
    storage.set(
        "get",
        Function::new(ctx.clone(), move |key: String| {
            storage_host.storage.borrow().get(&key).map(str::to_owned)
        })?,
    )?;
    let storage_host = host.clone();
    storage.set(
        "set",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, key: String, value: String| {
                storage_result(&ctx, storage_host.storage.borrow_mut().set(&key, &value))
            },
        )?,
    )?;
    let storage_host = host.clone();
    storage.set(
        "remove",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, key: String| {
            storage_result(&ctx, storage_host.storage.borrow_mut().remove(&key))
        })?,
    )?;
    let storage_host = host.clone();
    storage.set(
        "clear",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>| {
            storage_result(&ctx, storage_host.storage.borrow_mut().clear())
        })?,
    )?;
    let storage_host = host.clone();
    storage.set(
        "keys",
        Function::new(ctx.clone(), move || storage_host.storage.borrow().keys())?,
    )?;
    api.set("storage", storage)?;

    let log = Object::new(ctx.clone())?;
    for (name, level) in [
        ("debug", Level::Debug),
        ("info", Level::Info),
        ("warn", Level::Warn),
        ("error", Level::Error),
    ] {
        let log_host = host.clone();
        log.set(
            name,
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'js>, message: Value<'js>, fields: Opt<Value<'js>>| {
                    log_record(&ctx, &log_host, level, message, fields)
                },
            )?,
        )?;
    }
    api.set("log", log)?;
    ctx.globals().set("openkara", api)?;

    let timer_host = host.clone();
    ctx.globals().set(
        "setTimeout",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, callback: Function<'js>, delay: Opt<f64>| -> Result<u32> {
                let id = {
                    let mut timers = timer_host.timers.borrow_mut();
                    if timers.live.len() >= MAX_TIMERS {
                        return Err(Exception::throw_range(&ctx, "too many pending timers"));
                    }
                    timers.next = timers.next.wrapping_add(1).max(1);
                    let id = timers.next;
                    timers.live.insert(id);
                    id
                };
                let delay = delay
                    .0
                    .filter(|delay| delay.is_finite() && *delay > 0.0)
                    .unwrap_or(0.0)
                    .min(MAX_TIMER_DELAY_MS);
                let host = timer_host.clone();
                let context = ctx.clone();
                ctx.spawn(async move {
                    tokio::time::sleep(Duration::from_secs_f64(delay / 1000.0)).await;
                    if host.timers.borrow_mut().live.remove(&id)
                        && callback.call::<_, ()>(()).is_err()
                    {
                        context.catch();
                    }
                });
                Ok(id)
            },
        )?,
    )?;
    let timer_host = host.clone();
    ctx.globals().set(
        "clearTimeout",
        Function::new(ctx.clone(), move |id: Opt<Value<'js>>| {
            if let Some(id) = id.0.and_then(|id| id.as_number()) {
                timer_host.timers.borrow_mut().live.remove(&(id as u32));
            }
        })?,
    )?;
    Ok(())
}
