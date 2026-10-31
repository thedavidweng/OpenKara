# Deterministic Streaming Source package

This AGPL-3.0-only package implements the
[Streaming Source protocol](../../docs/references/contracts/streaming-source-protocol.md)
(protocol major 2) for the embedded engine (runtime `openkara-js`). It is
executable acceptance infrastructure, not a production music service. It
imports no host internals and needs no npm packages, user credentials, or
music vendor.

The package has one Streaming Playlist (`42`) and three tracks (`1`, `2`, `3`).
These ids deliberately collide with ids that other Streaming Sources use.
Their titles and audio differ. The default manifest declares playlist browse,
track details, and import. It does not declare search, liked tracks, or
preview. The first track page has one item; the second has two. Continuation
cursors are opaque, single-use, and bound to one engine run. Track `2` refuses
a trial clip. Tracks `1` and `3` resolve media through a lookup against the
declared API origin. Track artwork points at a private address, so the host
must refuse it and the catalog treats the cover as absent. No vendor media is
redistributed.

## Engine module (`source.mjs`)

`source.mjs` is one ES module. It has no Node APIs; it uses only the sandboxed
host object: `openkara.request` for media lookups against its declared API
origin, `openkara.storage` for the signed-in account marker, `openkara.log`,
and `setTimeout` in polling. Sign-in offers QR and text presentations (or a
web flow when the manifest says so), and a poll approves the test account on
its second call. A stale `session.restore` expires the session.

The host test support (`source_host::test_support::RegistryFixture`) bakes a
package manifest into the module's `__MANIFEST__` placeholder, packs the
result as an npm-style tarball, and serves it from a mock Source Registry, so
tests install it through the real registry download, integrity, and describe
checks. An inline copy would not exercise that path. `manifest.json` is the
default package manifest that the test support starts from.

## Installed application workflow tests

`catalog::plugin_tests` builds two packages from `source.mjs`. The
official-page package declares search, liked tracks, preview and import. The
challenge package declares playlist browsing and import, with QR and text
polling. Both use track `1` and playlist `42`, but return different
metadata, cursors, accounts and audio. Neither package is a production
discovery entry.

The catalog consumer test adds the mock registry, installs both packages
through `SourceHost`, and runs them in the embedded engine with no network.
The fixture's media lookups reach a local mock origin
(`HostConfig::add_fixture_upstream`); the public HTTPS media bytes use
constructor-injected public DNS addresses and fixture bytes. It does not
permit loopback media in production. Run from `src-tauri`:

```sh
cargo test --lib catalog::plugin_tests
```

This proves source-domain authentication verification, capability failures,
source-bound opaque pages and metadata, verified-owner credential isolation,
per-package KV storage across an engine restart, and the shared library writer
with different accepted audio. It does not prove native official-window cookie
capture, rendered UI, or distributed-app policy.

`source_host::media_tests` installs the default package the same way and
checks host media policy: declared public origins, refused private
destinations and headers, ranges, and known-expired media.

Keep the license and NOTICE in any published package. MIT transfer requires
the evidence in the
[rights inventory](../../docs/references/streaming-source-provenance.md).
