use super::test_support::{fixture_manifest, RegistryFixture};
use super::*;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const SOURCE: &str = r#"
let calls = 0;
const identity = {
  protocol: { major: 2, minor: 0 },
  source_id: "__SOURCE__",
  package_id: "org.openkara.__SOURCE__",
  package_version: "__VERSION__",
};
const attempts = {
  eval: () => eval("1 + 1"),
  indirect_eval: () => (0, globalThis.eval)("1 + 1"),
  function: () => Function("return 1")(),
  new_function: () => new Function("return 1")(),
  function_constructor: () => (function () {}).constructor("return 1")(),
  async_function: () => (async function () {}).constructor("return 1"),
  generator_function: () => (function* () {}).constructor("return 1"),
  async_generator_function: () => (async function* () {}).constructor("return 1"),
};
__TOP_LEVEL__
export default {
  describe() {
    __DESCRIBE__
    return {
      ...identity,
      name: "Engine Fixture",
      description: "Embedded engine test source.",
      capabilities: ["playlists"],
      authentication: { flow: "none" },
      requires_authentication: [],
      session: { state: "signed_out", account: null },
    };
  },
  async playlists({ cursor }) {
    calls += 1;
    switch (cursor) {
      case "spin":
        for (;;) {}
      case "never":
        return new Promise(() => {});
      case "grow": {
        const hoard = [];
        try {
          for (;;) hoard.push("x".repeat(1 << 16) + hoard.length);
        } catch (error) {
          return { items: [], next_cursor: null, caught: String(error) };
        }
      }
      case "recurse": {
        const deep = (depth) => deep(depth + 1) + 1;
        return deep(0);
      }
      case "eval": {
        const results = {};
        for (const [name, attempt] of Object.entries(attempts)) {
          try {
            attempt();
            results[name] = "ran";
          } catch (error) {
            results[name] = `${error.name}: ${error.message}`;
          }
        }
        return { items: [], next_cursor: null, results };
      }
      case "eval_uncaught":
        return eval("({ items: [], next_cursor: null })");
      case "not_found":
        throw { code: "not_found", message: "No such playlist." };
      case "leak":
        globalThis.leak = "__SOURCE__";
        break;
      case "peek":
        return { items: [], next_cursor: null, leak: typeof globalThis.leak };
    }
    return {
      items: [
        { id: "42", name: "Fixture playlist", description: null, artwork: null, track_count: null },
      ],
      next_cursor: null,
      calls,
    };
  },
};
"#;

#[derive(Default)]
struct Package {
    source_id: &'static str,
    top_level: &'static str,
    describe: &'static str,
    protocol_major: Option<u32>,
}
impl Package {
    fn new(source_id: &'static str) -> Self {
        Self {
            source_id,
            ..Self::default()
        }
    }
    fn fixture(&self) -> (SourceManifest, String) {
        let mut manifest = fixture_manifest(self.source_id, "1.0.0");
        manifest.runtime = RuntimeRequirement {
            kind: "openkara-js".into(),
            major: 1,
        };
        manifest.protocol = ProtocolDeclaration {
            majors: vec![self.protocol_major.unwrap_or(2)],
            minor: 0,
        };
        manifest.entry = "source.mjs".into();
        manifest.capabilities = vec!["playlists".into()];
        manifest.authentication = json!({"flow":"none"});
        manifest.requires_authentication = vec![];
        let source = SOURCE
            .replace("__SOURCE__", self.source_id)
            .replace("__VERSION__", &manifest.version)
            .replace("__TOP_LEVEL__", self.top_level)
            .replace("__DESCRIBE__", self.describe);
        (manifest, source)
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    registry: RegistryFixture,
    host: SourceHost,
}
impl Fixture {
    async fn new(packages: Vec<Package>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut registry = RegistryFixture::new("org.openkara").await;
        let sources: Vec<_> = packages.iter().map(Package::fixture).collect();
        registry.publish_sources(&sources).await;
        let mut config = HostConfig::fixture(root.path().to_path_buf(), "0.17.0".into());
        config.describe_timeout = Duration::from_millis(1500);
        config.operation_timeout = Duration::from_millis(300);
        config.engine_memory_limit = 16 * 1024 * 1024;
        let host = SourceHost::new(config).await.unwrap();
        host.add_registry(&registry.url()).await.unwrap();
        Self {
            _root: root,
            registry,
            host,
        }
    }
    async fn install(&self, id: &str) -> HostResult<SourceSnapshot> {
        self.host
            .install_package(
                self.registry.registry_id(),
                &self.registry.package_id(id),
                None,
                false,
                false,
            )
            .await
    }
    async fn enabled(packages: Vec<Package>) -> Self {
        let ids: Vec<_> = packages.iter().map(|package| package.source_id).collect();
        let fixture = Self::new(packages).await;
        for id in ids {
            fixture.install(id).await.unwrap();
            fixture.host.set_enabled(id, true, false).await.unwrap();
        }
        fixture
    }
    async fn playlists(&self, id: &str, cursor: &str) -> HostResult<Value> {
        self.host
            .request(id, "playlists", json!({"cursor":cursor}))
            .await
    }
}

#[tokio::test]
async fn embedded_source_describes_and_pages_playlists_without_node() {
    let fixture = Fixture::enabled(vec![Package::new("engine")]).await;
    let host = &fixture.host;
    let snapshot = host.snapshot("engine").unwrap();
    assert_eq!(snapshot.installed_version.as_deref(), Some("1.0.0"));
    assert_eq!(snapshot.state, SourceState::Ready);

    let description = host.request("engine", "describe", json!({})).await.unwrap();
    assert_eq!(description["name"], "Engine Fixture");
    assert_eq!(description["session"]["state"], "signed_out");
    let page = host
        .request("engine", "playlists", json!({"cursor":null}))
        .await
        .unwrap();
    assert_eq!(page["items"][0]["id"], "42");
    assert_eq!(page["next_cursor"], Value::Null);
    let again = fixture.playlists("engine", "next").await.unwrap();
    assert_eq!(again["calls"], 2, "one runtime serves consecutive calls");
    assert!(host.snapshot("engine").unwrap().running);

    assert_eq!(
        fixture
            .playlists("engine", "not_found")
            .await
            .unwrap_err()
            .code,
        "not_found"
    );
    assert_eq!(
        host.request("engine", "search", json!({"query":"x","cursor":null}))
            .await
            .unwrap_err()
            .code,
        "unsupported_operation"
    );
}

#[tokio::test]
async fn endless_loop_hits_the_deadline_and_the_next_call_starts_fresh() {
    let fixture = Fixture::enabled(vec![Package::new("engine")]).await;
    let host = &fixture.host;
    assert_eq!(
        fixture.playlists("engine", "warm").await.unwrap()["calls"],
        1
    );

    let started = Instant::now();
    let spinning = host
        .start_request("engine", "playlists", json!({"cursor":"spin"}))
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_millis(200),
        tokio::time::sleep(Duration::from_millis(20)),
    )
    .await
    .expect("the engine runs off the caller's async runtime");
    assert!(host.snapshot("engine").unwrap().running);
    assert_eq!(
        spinning.result().await.unwrap_err().code,
        "source_unavailable"
    );
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(300), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    assert!(!host.snapshot("engine").unwrap().running);

    assert_eq!(
        fixture.playlists("engine", "warm").await.unwrap()["calls"],
        1,
        "a stopped source restarts with fresh state"
    );
    assert_eq!(
        fixture.playlists("engine", "never").await.unwrap_err().code,
        "source_unavailable"
    );
    assert_eq!(
        fixture.playlists("engine", "warm").await.unwrap()["calls"],
        1
    );
}

#[tokio::test]
async fn memory_and_stack_blow_ups_stop_the_source_even_when_caught() {
    let fixture = Fixture::enabled(vec![Package::new("engine")]).await;
    for cursor in ["grow", "recurse", "grow"] {
        assert_eq!(
            fixture.playlists("engine", "warm").await.unwrap()["calls"],
            1
        );
        assert_eq!(
            fixture.playlists("engine", cursor).await.unwrap_err().code,
            "source_unavailable",
            "{cursor}"
        );
        assert!(!fixture.host.snapshot("engine").unwrap().running);
    }
    assert_eq!(
        fixture.playlists("engine", "warm").await.unwrap()["calls"],
        1
    );
}

#[tokio::test]
async fn eval_and_function_constructors_are_rejected() {
    let fixture = Fixture::enabled(vec![Package::new("engine")]).await;
    let page = fixture.playlists("engine", "eval").await.unwrap();
    let results = page["results"].as_object().unwrap();
    assert_eq!(results.len(), 8);
    for (attempt, outcome) in results {
        let outcome = outcome.as_str().unwrap();
        assert!(outcome.starts_with("TypeError"), "{attempt}: {outcome}");
    }
    assert_eq!(
        fixture
            .playlists("engine", "eval_uncaught")
            .await
            .unwrap_err()
            .code,
        "internal"
    );
    assert_eq!(
        fixture.playlists("engine", "warm").await.unwrap()["calls"],
        3,
        "a rejected eval does not restart the source"
    );
}

#[tokio::test]
async fn sources_share_no_globals() {
    let fixture = Fixture::enabled(vec![Package::new("first"), Package::new("second")]).await;
    fixture.playlists("first", "leak").await.unwrap();
    assert_eq!(
        fixture.playlists("first", "peek").await.unwrap()["leak"],
        "string"
    );
    assert_eq!(
        fixture.playlists("second", "peek").await.unwrap()["leak"],
        "undefined"
    );
}

#[tokio::test]
async fn describe_deadline_and_compatibility_check_gate_installation() {
    let fixture = Fixture::new(vec![
        Package {
            describe: "for (;;) {}",
            ..Package::new("spin_describe")
        },
        Package {
            top_level: "for (;;) {}",
            ..Package::new("spin_load")
        },
        Package {
            describe: r#"return { ...identity, package_version: "9.9.9" };"#,
            ..Package::new("mismatch")
        },
        Package {
            protocol_major: Some(1),
            ..Package::new("old_protocol")
        },
    ])
    .await;
    let host = &fixture.host;
    for (id, code) in [
        ("spin_describe", "source_unavailable"),
        ("spin_load", "source_unavailable"),
        ("mismatch", "incompatible_plugin"),
        ("old_protocol", "incompatible_plugin"),
    ] {
        let started = Instant::now();
        assert_eq!(fixture.install(id).await.unwrap_err().code, code, "{id}");
        assert!(started.elapsed() < Duration::from_secs(5), "{id}");
        assert_eq!(host.snapshot(id).unwrap().installed_version, None, "{id}");
    }
}
