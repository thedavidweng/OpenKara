# Deterministic Streaming Source package

This AGPL-3.0-only package implements
[protocol 1.0](../../docs/references/contracts/streaming-source-protocol.md).
It is executable acceptance infrastructure, not a production music service.
It imports no host internals and needs no npm packages, user credentials, or
music vendor. `worker.mjs` is its stdio entry. Node 24 is the runtime family;
release signing must pin an exact runtime and platform digests in the index.

The package has one Streaming Playlist (`42`) and three tracks (`1`, `2`, `3`).
These ids deliberately collide with the existing NetEase behavior fixtures.
Their titles and audio differ. It supports playlist browse, track details,
and import. It does not support search, liked tracks, or preview. The first
track page has one item; the second has two. Continuation cursors are opaque,
single-use, and session-bound. Track `2` refuses a trial clip. Tracks `1` and
`3` resolve full one-second generated WAV tones. The HTTPS server generates
both audio and PNG artwork; no vendor media is redistributed.

## Real stdio smoke

From the application repository root, with Node 24 available for development:

```sh
node fixtures/streaming-source/smoke.mjs
OPENKARA_FIXTURE_SCENARIO=text node fixtures/streaming-source/smoke.mjs
OPENKARA_FIXTURE_SCENARIO=expired_challenge node fixtures/streaming-source/smoke.mjs
OPENKARA_FIXTURE_SCENARIO=network node fixtures/streaming-source/smoke.mjs
```

Each command spawns the real worker process with stdin/stdout pipes. There is
no mock API, echo worker, or protocol replacement. Assertions check the known
fixture domain results, not source text or helper internals. Exit status must
be zero. Normal/text output starts with `PASS real stdio: handshake, capabilities`
and covers sign-in, uneven pages, refused media, full media metadata, correlated
progress/cancellation, sign-out, restore, and explicit session expiry. Expiry
output is `PASS real stdio: expired challenge remains unauthenticated`.
Network output is `PASS real stdio: network failure is distinct from session expiry and refusal`.

The smoke checks process behavior. It is not application-host integration or a
permanent test suite at a new seam. The approved permanent behavioral seam
remains the application source workflow with the real host. This client does
not establish library outcomes, signature validation, or platform acceptance.

Synchronous session and cursor operations commit their terminal outcome before
a later cancellation can win. Authentication cancellation and sign-out remain
available when sixteen media requests are pending. BOM-prefixed frames fail
the process without a success response, as the transport contract requires.

The process can also be driven directly:

```sh
node fixtures/streaming-source/worker.mjs
```

Send a `handshake` frame, then `auth.start`. Render the returned QR content or
text as data. Poll its challenge twice: pending, then the verified test account
and opaque credentials. Cancelled challenges stay cancelled. In the
`expired_challenge` scenario they expire without sign-in. `network` rejects
browse/resolve only after authentication, so the host can distinguish upstream
failure from expiry. Environment scenarios are test setup, not protocol
capabilities or a user-facing vendor setting.

## Installed application workflow tests

`workflow-worker.mjs` implements two manifest-driven test packages. The official-page
package declares search, liked tracks, preview and import. The challenge package
declares playlist browsing and import, with QR polling. Both use track `1` and
playlist `42`, but return different metadata, cursors, accounts and audio.
Neither package is a production discovery entry.

The catalog consumer test builds both archives, signs an index with independent
ephemeral keys, installs them through `SourceHost`, and runs their real stdio
processes. Its exact public HTTPS media URLs use constructor-injected public DNS
addresses and fixture bytes. It does not permit loopback media in production.
Prepare the exact Node 24.22.1 runtime, then run from `src-tauri`:

```sh
OPENKARA_SOURCE_TEST_NODE=/absolute/path/to/node \
  cargo test --lib catalog::plugin_tests -- --ignored --nocapture
```

This proves source-domain authentication verification, capability failures,
source-bound opaque pages and metadata, verified-owner credential isolation,
and the shared library writer with different accepted audio. It does not prove
native official-window cookie capture, rendered UI, or distributed-app policy.

## Real media smoke

The default manifest declares `https://streaming-fixture.openkara.test:9443`.
For a local media-only smoke, create an ephemeral certificate outside the repo:

```sh
openssl req -x509 -newkey rsa:2048 -nodes -days 1 \
  -keyout /tmp/openkara-fixture-key.pem -out /tmp/openkara-fixture-cert.pem \
  -subj /CN=streaming-fixture.openkara.test \
  -addext subjectAltName=DNS:streaming-fixture.openkara.test
node fixtures/streaming-source/media-server.mjs \
  /tmp/openkara-fixture-key.pem /tmp/openkara-fixture-cert.pem
```

In another terminal:

```sh
curl --noproxy '*' --fail --cacert /tmp/openkara-fixture-cert.pem \
  --resolve streaming-fixture.openkara.test:9443:127.0.0.1 \
  https://streaming-fixture.openkara.test:9443/tone-1.wav \
  --output /tmp/openkara-fixture-tone.wav
curl --noproxy '*' --fail --head --cacert /tmp/openkara-fixture-cert.pem \
  --resolve streaming-fixture.openkara.test:9443:127.0.0.1 \
  --header 'Range: bytes=44-143' \
  https://streaming-fixture.openkara.test:9443/tone-3.wav
```

Expected: the first request writes 16044 bytes of PCM WAV. The second returns
206, `Content-Range: bytes 44-143/16044`, and `Content-Length: 100`.
`/artwork.png` serves a generated 32-by-32 PNG. Unknown paths return 404;
invalid or unsatisfiable byte ranges return 416. No `--insecure` is needed.
Stop the server and remove these temporary files after the smoke.

The local endpoint is only a media-server smoke. A production host must reject
its private-network destination. For later real host imports, deploy this
server behind a publicly reachable, trusted HTTPS test origin. Set both origin
arrays in the fixture manifest to that exact origin before packaging/signing.
The worker reads its manifest and uses that origin for all descriptors. Serve
the same paths through the reverse proxy. Do not loosen the host URL validator,
add a source-specific application branch, or claim a local curl proves import.
That deployment is an external prerequisite for full accepted-media host tests.
Refusal, auth, capability, and pagination checks need no media endpoint.

## Archive for later host tests

No client-side dependency installation is needed. The lockfile has no external
dependencies. To build a versioned development archive:

```sh
tar -czf /tmp/openkara-streaming-fixture-1.0.0.tar.gz \
  -C fixtures/streaming-source \
  manifest.json worker.mjs package.json package-lock.json LICENSE NOTICE
```

Keep archive contents at the root. The media server and smoke client are harness
infrastructure, not plugin runtime dependencies. A host acceptance harness must
sign the archive/index with its designated test key and bind its exact digest,
size, identities, runtime pin and compatibility. This repository does not ship
production signing secrets or a fake production signature. Never label a raw
tarball installed or trusted. Keep the license/NOTICE in published packages.
MIT transfer requires the evidence in the
[rights inventory](../../docs/references/streaming-source-provenance.md).
