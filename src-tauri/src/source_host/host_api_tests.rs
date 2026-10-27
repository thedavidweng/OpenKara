use super::test_support::{fixture_manifest, FixtureDistribution, FixturePackage};
use super::*;
use crate::catalog::StreamingSource;
use mockito::{Matcher, Server, ServerGuard};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

const API: &str = "https://api.fixture.test";
const MEDIA: &str = "https://media.fixture.test";
const UNDECLARED: &str = "https://undeclared.fixture.test";

const SOURCE: &str = r#"
let restored = null;
const identity = {
  protocol: { major: 2, minor: 0 },
  source_id: "__SOURCE__",
  package_id: "org.openkara.__SOURCE__",
  package_version: "1.0.0",
};
const session = () =>
  restored
    ? { state: "signed_in", account: { id: "1", display_name: "__SOURCE__ account" } }
    : { state: "signed_out", account: null };
const attempt = async (run) => {
  try {
    return { ok: await run() };
  } catch (error) {
    return { code: error.code ?? null, name: error.name, message: String(error.message) };
  }
};
const page = (extra) => ({ items: [], next_cursor: null, ...extra });
export default {
  describe() {
    return {
      ...identity,
      name: "Host API Fixture",
      description: "Host API test source.",
      capabilities: ["playlists"],
      authentication: { flow: "plugin", presentations: ["qr"] },
      requires_authentication: [],
      session: session(),
    };
  },
  "session.restore"({ credentials }) {
    restored = credentials;
    return session();
  },
  "auth.sign_out"() {
    restored = null;
    return session();
  },
  async playlists(params) {
    const { crypto, encoding, storage, log } = openkara;
    switch (params.cursor) {
      case "request": {
        const result = await attempt(() => openkara.request(params.options));
        if (result.ok && result.ok.body instanceof Uint8Array) {
          result.ok.body = { bytes: result.ok.body.length, first: result.ok.body[0] };
        }
        return page({ result });
      }
      case "crypto": {
        const key = encoding.hexDecode("2b7e151628aed2a6abf7158809cf4f3c");
        const iv = encoding.hexDecode("000102030405060708090a0b0c0d0e0f");
        const sealed = crypto.aesEncrypt({ mode: "cbc", key, iv }, "hello, sandbox");
        const gcm = { mode: "gcm", key, iv: iv.slice(0, 12), aad: "header" };
        return page({
          sha256: encoding.hexEncode(crypto.digest("sha256", "abc")),
          md5: encoding.hexEncode(crypto.digest("md5", "abc")),
          sha1: encoding.hexEncode(crypto.digest("sha1", new Uint8Array([0x61, 0x62, 0x63]))),
          hmac: encoding.hexEncode(crypto.hmac("sha256", "Jefe", "what do ya want for nothing?")),
          ecb: encoding.hexEncode(
            crypto.aesEncrypt(
              { mode: "ecb", key, padding: "none" },
              encoding.hexDecode("6bc1bee22e409f96e93d7e117393172a"),
            ),
          ),
          cbc: encoding.utf8Decode(crypto.aesDecrypt({ mode: "cbc", key, iv }, sealed)),
          gcm: encoding.utf8Decode(crypto.aesDecrypt(gcm, crypto.aesEncrypt(gcm, "sealed"))),
          rsa: encoding.hexEncode(
            crypto.rsaPublic(
              { modulus: encoding.hexDecode(params.modulus), exponent: encoding.hexDecode("010001") },
              encoding.hexDecode("02"),
            ),
          ),
          random: [crypto.randomBytes(16).length, encoding.hexEncode(crypto.randomBytes(8)).length],
          base64: encoding.base64Encode(new Uint8Array([0xfb, 0xff, 0x00, 0x10])),
          base64url: encoding.base64Encode(new Uint8Array([0xfb, 0xff, 0x00, 0x10]), {
            urlSafe: true,
            padding: false,
          }),
          decoded: Array.from(encoding.base64Decode("-_8AEA")),
          bad_key: await attempt(() => crypto.aesEncrypt({ mode: "cbc", key: "short", iv }, "x")),
          bad_digest: await attempt(() => crypto.digest("sha3", "x")),
        });
      }
      case "storage_write":
        storage.set("device", "abc");
        storage.set("scratch", "1");
        storage.remove("scratch");
        return page({
          keys: storage.keys(),
          oversized: await attempt(() => storage.set("big", "x".repeat(1100000))),
          not_text: await attempt(() => storage.set("number", 7)),
        });
      case "storage_read":
        return page({ device: storage.get("device"), missing: storage.get("missing"), keys: storage.keys() });
      case "timers": {
        const order = [];
        const cancelled = setTimeout(() => order.push("cancelled"), 5);
        clearTimeout(cancelled);
        await new Promise((resolve) => setTimeout(() => resolve(order.push("fired")), 20));
        await new Promise((resolve) => setTimeout(resolve, 10));
        return page({ order });
      }
      case "log": {
        const token = restored ? restored.token : "none";
        log.info(`restored ${token} via https://api.fixture.test/login?sig=signed-value`, {
          cookie: "session=abc",
          note: `token ${token}`,
          playlist: "42",
        });
        log.warn({ structured: true });
        return page();
      }
      case "probe":
        return page({
          restored,
          storage: storage.keys(),
          globals: Object.getOwnPropertyNames(globalThis).filter((name) =>
            /credential|secret|session|keychain|cookie/i.test(name),
          ),
          request: await attempt(() => openkara.request({ url: params.url })),
        });
    }
    return page();
  },
};
"#;

fn package(source_id: &str, origins: &[&str], media: &[&str]) -> FixturePackage {
    let mut manifest = fixture_manifest(source_id, "1.0.0");
    manifest.runtime = RuntimeRequirement {
        kind: "openkara-js".into(),
        major: 1,
    };
    manifest.protocol = ProtocolDeclaration {
        majors: vec![2],
        minor: 0,
    };
    manifest.entry = "source.mjs".into();
    manifest.capabilities = vec!["playlists".into()];
    manifest.authentication = json!({"flow":"plugin","presentations":["qr"]});
    manifest.requires_authentication = vec![];
    manifest.resource_origins = origins.iter().map(|origin| origin.to_string()).collect();
    manifest.media_origins = media.iter().map(|origin| origin.to_string()).collect();
    FixturePackage {
        worker: SOURCE.replace("__SOURCE__", source_id),
        manifest,
        revoked_versions: vec![],
    }
}

struct Fixture {
    root: tempfile::TempDir,
    _distribution: FixtureDistribution,
    host: SourceHost,
}
impl Fixture {
    async fn new(packages: Vec<FixturePackage>, upstreams: &[(&str, &ServerGuard)]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let ids: Vec<_> = packages
            .iter()
            .map(|package| package.manifest.source_id.clone())
            .collect();
        let mut distribution = FixtureDistribution::new().await;
        distribution.publish(packages).await;
        let mut config = distribution.engine_config(root.path().join("host"));
        config.describe_timeout = Duration::from_secs(5);
        config.operation_timeout = Duration::from_secs(10);
        config.backoff_unit = Duration::from_millis(1);
        config.engine_memory_limit = 32 * 1024 * 1024;
        for (origin, server) in upstreams {
            config.add_fixture_upstream(origin.to_string(), server.url());
        }
        let host = SourceHost::new(config).await.unwrap();
        host.refresh().await.unwrap();
        for id in &ids {
            host.install(id, None, false).await.unwrap();
            host.set_enabled(id, true, false).await.unwrap();
        }
        Self {
            root,
            _distribution: distribution,
            host,
        }
    }
    async fn call(&self, id: &str, params: Value) -> Value {
        self.host.request(id, "playlists", params).await.unwrap()
    }
    async fn fetch(&self, id: &str, options: Value) -> Value {
        self.call(id, json!({"cursor":"request","options":options}))
            .await["result"]
            .take()
    }
}

#[tokio::test]
async fn requests_reach_declared_origins_and_carry_only_that_sources_cookies() {
    let mut api = Server::new_async().await;
    let mut media = Server::new_async().await;
    let login = api
        .mock("GET", "/login")
        .with_header("set-cookie", "session=jar-value-123; Path=/")
        .with_header("content-type", "application/json")
        .with_body(r#"{"signed_in":true}"#)
        .expect(1)
        .create_async()
        .await;
    let with_jar = api
        .mock("POST", "/echo")
        .match_header("cookie", "explicit=1; session=jar-value-123")
        .match_header("x-real-ip", "203.0.113.7")
        .match_body("payload")
        .with_body("echoed")
        .expect(1)
        .create_async()
        .await;
    let without_jar = api
        .mock("POST", "/echo")
        .match_header("cookie", Matcher::Missing)
        .with_body("no cookies")
        .expect(1)
        .create_async()
        .await;
    let bytes = media
        .mock("GET", "/blob")
        .with_body([7u8, 8, 9])
        .expect(1)
        .create_async()
        .await;
    let fixture = Fixture::new(
        vec![
            package("first", &[API], &[MEDIA]),
            package("second", &[API], &[]),
        ],
        &[(API, &api), (MEDIA, &media)],
    )
    .await;

    let signed_in = fixture
        .fetch(
            "first",
            json!({"url":format!("{API}/login"),"responseType":"json"}),
        )
        .await;
    assert_eq!(signed_in["ok"]["status"], 200);
    assert_eq!(signed_in["ok"]["url"], format!("{API}/login"));
    assert_eq!(signed_in["ok"]["body"], json!({"signed_in":true}));
    assert_eq!(
        signed_in["ok"]["headers"]["content-type"],
        "application/json"
    );
    assert_eq!(
        signed_in["ok"]["setCookies"],
        json!(["session=jar-value-123; Path=/"])
    );

    let echoed = fixture
        .fetch(
            "first",
            json!({"url":format!("{API}/echo"),"method":"POST","body":"payload",
                   "headers":{"Cookie":"explicit=1","X-Real-IP":"203.0.113.7"}}),
        )
        .await;
    assert_eq!(echoed["ok"]["body"], "echoed", "{echoed}");
    let other = fixture
        .fetch(
            "second",
            json!({"url":format!("{API}/echo"),"method":"POST","body":"payload"}),
        )
        .await;
    assert_eq!(other["ok"]["body"], "no cookies", "{other}");
    let blob = fixture
        .fetch(
            "first",
            json!({"url":format!("{MEDIA}/blob"),"responseType":"bytes"}),
        )
        .await;
    assert_eq!(blob["ok"]["body"], json!({"bytes":3,"first":7}));
    for mock in [login, with_jar, without_jar, bytes] {
        mock.assert_async().await;
    }

    fixture
        .host
        .request("first", "auth.sign_out", json!({}))
        .await
        .unwrap();
    let after_sign_out = api
        .mock("GET", "/after")
        .match_header("cookie", Matcher::Missing)
        .with_body("forgotten")
        .expect(1)
        .create_async()
        .await;
    let forgotten = fixture
        .fetch("first", json!({"url":format!("{API}/after")}))
        .await;
    assert_eq!(forgotten["ok"]["body"], "forgotten", "{forgotten}");
    after_sign_out.assert_async().await;
}

#[tokio::test]
async fn requests_to_undeclared_origins_never_leave_the_host() {
    let mut api = Server::new_async().await;
    let mut undeclared = Server::new_async().await;
    let reached = undeclared
        .mock("GET", Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let forbidden_header = api.mock("GET", "/x").expect(0).create_async().await;
    let fixture = Fixture::new(
        vec![package("engine", &[API], &[])],
        &[(API, &api), (UNDECLARED, &undeclared)],
    )
    .await;
    for options in [
        json!({"url":format!("{UNDECLARED}/steal")}),
        json!({"url":"https://sub.api.fixture.test/x"}),
        json!({"url":"https://api.fixture.test:8443/x"}),
        json!({"url":"http://api.fixture.test/x"}),
        json!({"url":"https://user@api.fixture.test/x"}),
        json!({"url":format!("{API}/x"),"headers":{"Host":"undeclared.fixture.test"}}),
        json!({"url":format!("{API}/x"),"headers":{"Referer":"http://plain.fixture.test/"}}),
        json!({"url":format!("{API}/x"),"method":"CONNECT"}),
    ] {
        let refused = fixture.fetch("engine", options.clone()).await;
        assert_eq!(refused["code"], "invalid_resource", "{options}: {refused}");
    }
    reached.assert_async().await;
    forbidden_header.assert_async().await;
}

#[tokio::test]
async fn redirects_are_checked_at_every_hop() {
    let mut api = Server::new_async().await;
    let mut media = Server::new_async().await;
    let mut undeclared = Server::new_async().await;
    api.mock("GET", "/escape")
        .with_status(302)
        .with_header("location", &format!("{UNDECLARED}/steal"))
        .create_async()
        .await;
    api.mock("POST", "/bounce")
        .with_status(303)
        .with_header("location", &format!("{MEDIA}/landing"))
        .create_async()
        .await;
    let reached = undeclared
        .mock("GET", Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let landing = media
        .mock("GET", "/landing")
        .match_header("cookie", Matcher::Missing)
        .match_header("authorization", Matcher::Missing)
        .match_header("x-trace", "kept")
        .with_body("landed")
        .expect(1)
        .create_async()
        .await;
    let fixture = Fixture::new(
        vec![package("engine", &[API], &[MEDIA])],
        &[(API, &api), (MEDIA, &media), (UNDECLARED, &undeclared)],
    )
    .await;

    let escaped = fixture
        .fetch("engine", json!({"url":format!("{API}/escape")}))
        .await;
    assert_eq!(escaped["code"], "invalid_resource", "{escaped}");
    let landed = fixture
        .fetch(
            "engine",
            json!({"url":format!("{API}/bounce"),"method":"POST","body":"form",
                   "headers":{"Cookie":"a=b","Authorization":"Bearer t0ken","X-Trace":"kept"}}),
        )
        .await;
    assert_eq!(landed["ok"]["body"], "landed", "{landed}");
    assert_eq!(landed["ok"]["url"], format!("{MEDIA}/landing"));
    reached.assert_async().await;
    landing.assert_async().await;
}

#[tokio::test]
async fn oversized_responses_are_refused() {
    let mut api = Server::new_async().await;
    api.mock("GET", "/large")
        .with_body(vec![b'x'; network::MAX_SOURCE_RESPONSE as usize + 1])
        .create_async()
        .await;
    api.mock("GET", "/fits")
        .with_body(vec![b'x'; 1024])
        .create_async()
        .await;
    let fixture = Fixture::new(vec![package("engine", &[API], &[])], &[(API, &api)]).await;
    let refused = fixture
        .fetch("engine", json!({"url":format!("{API}/large")}))
        .await;
    assert_eq!(refused["code"], "invalid_resource", "{refused}");
    let fits = fixture
        .fetch(
            "engine",
            json!({"url":format!("{API}/fits"),"responseType":"bytes"}),
        )
        .await;
    assert_eq!(fits["ok"]["body"]["bytes"], 1024);
}

#[tokio::test]
async fn crypto_encoding_storage_and_timers_work_inside_the_sandbox() {
    let modulus = concat!(
        "e0b509f6259df8642dbc35662901477df22677ec152b5ff68ace615bb7b72594",
        "21a8ff2ad3b8b6b2d04a2f0d9d7ba7e2c0d4d0f5c1e9c1b5c8f8a5a8c8a7d4c3"
    );
    let fixture = Fixture::new(vec![package("engine", &[API], &[])], &[]).await;
    let results = fixture
        .call("engine", json!({"cursor":"crypto","modulus":modulus}))
        .await;
    assert_eq!(
        results["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(results["md5"], "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(results["sha1"], "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(
        results["hmac"],
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    assert_eq!(results["ecb"], "3ad77bb40d7a3660a89ecaf32466ef97");
    assert_eq!(results["cbc"], "hello, sandbox");
    assert_eq!(results["gcm"], "sealed");
    let modulus = num_bigint::BigUint::parse_bytes(modulus.as_bytes(), 16).unwrap();
    let expected = num_bigint::BigUint::from(2u8)
        .modpow(&num_bigint::BigUint::from(65537u32), &modulus)
        .to_str_radix(16);
    assert_eq!(results["rsa"], format!("{expected:0>128}"));
    assert_eq!(results["random"], json!([16, 16]));
    assert_eq!(results["base64"], "+/8AEA==");
    assert_eq!(results["base64url"], "-_8AEA");
    assert_eq!(results["decoded"], json!([251, 255, 0, 16]));
    assert_eq!(results["bad_key"]["name"], "TypeError");
    assert_eq!(results["bad_digest"]["name"], "TypeError");

    let written = fixture
        .call("engine", json!({"cursor":"storage_write"}))
        .await;
    assert_eq!(written["keys"], json!(["device"]));
    assert_eq!(written["oversized"]["message"], "source storage is full");
    assert_eq!(written["not_text"]["name"], "TypeError", "{written}");
    fixture
        .host
        .set_enabled("engine", false, false)
        .await
        .unwrap();
    fixture
        .host
        .set_enabled("engine", true, false)
        .await
        .unwrap();
    let read = fixture
        .call("engine", json!({"cursor":"storage_read"}))
        .await;
    assert_eq!(read["device"], "abc", "storage survives a restart");
    assert_eq!(read["missing"], Value::Null);
    assert_eq!(read["keys"], json!(["device"]));

    let timers = fixture.call("engine", json!({"cursor":"timers"})).await;
    assert_eq!(timers["order"], json!(["fired"]));
}

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn source_logs_are_structured_and_redacted() {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_max_level(tracing::Level::DEBUG)
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let fixture = Fixture::new(vec![package("engine", &[API], &[])], &[]).await;
    fixture
        .host
        .request(
            "engine",
            "session.restore",
            json!({"credentials":{"token":"super-secret-token"}}),
        )
        .await
        .unwrap();
    fixture.call("engine", json!({"cursor":"log"})).await;

    let output = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    let records: Vec<_> = output
        .lines()
        .filter(|line| line.contains("openkara::streaming_source"))
        .collect();
    assert_eq!(records.len(), 2, "{output}");
    assert!(records[0].contains("INFO"), "{output}");
    assert!(records[0].contains("source=\"engine\""), "{output}");
    assert!(
        records[0].contains("restored [redacted] via https://api.fixture.test/login?[redacted]"),
        "{output}"
    );
    assert!(records[0].contains(r#""cookie":"[redacted]""#), "{output}");
    assert!(records[0].contains(r#""playlist":"42""#), "{output}");
    assert!(records[1].contains("WARN"), "{output}");
    assert!(records[1].contains(r#"{"structured":true}"#), "{output}");
    for secret in ["super-secret-token", "signed-value", "session=abc"] {
        assert!(!output.contains(secret), "{secret} leaked: {output}");
    }
}

#[tokio::test]
async fn a_source_cannot_read_another_sources_credentials() {
    let mut victim_api = Server::new_async().await;
    let reached = victim_api
        .mock("GET", Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let victim_origin = "https://victim.fixture.test";
    let thief_origin = "https://thief.fixture.test";
    let fixture = Fixture::new(
        vec![
            package("victim", &[victim_origin], &[]),
            package("thief", &[thief_origin], &[]),
        ],
        &[(victim_origin, &victim_api)],
    )
    .await;
    let root = fixture.root.path().to_path_buf();
    let victim_owner = fixture.host.description("victim").unwrap().owner_binding;
    let victim_credentials =
        crate::catalog::StreamingCredentials::from([("token".into(), "victim-token".into())]);
    crate::catalog::store_session(&root, &victim_owner, victim_credentials).unwrap();
    // A package that held the `thief` source id earlier left credentials behind.
    crate::catalog::store_legacy_session(
        &root,
        "thief",
        "earlier-publisher/earlier-package",
        crate::catalog::StreamingCredentials::from([("token".into(), "earlier-token".into())]),
    )
    .unwrap();

    let host = fixture.host.clone();
    let sessions = tokio::task::spawn_blocking(move || {
        ["victim", "thief"].map(|id| {
            crate::catalog::PluginStreamingSource::open(host.clone(), id.into(), root.clone())
                .unwrap()
                .session()
                .unwrap()
                .signed_in
        })
    })
    .await
    .unwrap();
    assert_eq!(sessions, [true, false]);

    let victim = fixture
        .call(
            "victim",
            json!({"cursor":"probe","url":format!("{thief_origin}/me")}),
        )
        .await;
    assert_eq!(victim["restored"], json!({"token":"victim-token"}));
    let thief = fixture
        .call(
            "thief",
            json!({"cursor":"probe","url":format!("{victim_origin}/me")}),
        )
        .await;
    assert_eq!(thief["restored"], Value::Null, "{thief}");
    assert_eq!(thief["storage"], json!([]));
    assert_eq!(thief["globals"], json!([]));
    assert_eq!(thief["request"]["code"], "invalid_resource", "{thief}");
    reached.assert_async().await;
}
