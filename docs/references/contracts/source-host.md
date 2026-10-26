# Streaming Source Host

The app-independent Rust module `source_host` owns reviewed source discovery,
installation, and process lifecycle. The application adapter owns credentials,
authentication windows, preview, import, and library writes. See
[ADR 0039](../../adr/0039-install-streaming-sources-as-independent-trusted-packages.md)
and the [source protocol](streaming-source-protocol.md).

## Signed distribution

Production discovery reads
`https://github.com/thedavidweng/openkara-sources/releases/latest/download/source-index.json`.
The source publication work must create this repository and asset. Until then,
discovery returns a network error. The application does not invent catalog
entries or installed state.

The UTF-8 JSON envelope has three required string fields:

- `key_id`: an embedded dedicated source-signing key identifier.
- `payload`: standard base64 of the exact UTF-8 index JSON bytes.
- `signature`: standard base64 of the Ed25519 signature over those decoded bytes.

The host verifies the signature before it parses release data. It accepts these
public roots. They are separate from the application updater key:

| Slot    | Key identifier     | Raw 32-byte public key, standard base64        |
| ------- | ------------------ | ---------------------------------------------- |
| Current | `0374d430d3cf1aa6` | `SGkXN6xbau63VKXOSU8kGVJtMN0DhxFDmxlFvelqUXU=` |
| Next    | `71a135edb0fedcf0` | `VfvOxNOgYgIbKkzMDr8cLgqKIUbH2H9QcsSlc8qc28w=` |

The index payload has this shape:

```text
{
  schema_version: 1,
  generation: unsigned integer,
  sources: [{
    source_id, publisher, package_id,
    revoked_versions: [SemVer string],
    releases: [{
      manifest: complete protocol SourceManifest,
      archive: {url, size, sha256, format},
      runtime: {
        version: exact Node SemVer without a leading v,
        targets: {
          "OS-ARCH": {url, size, sha256, format, executable}
        }
      }
    }]
  }]
}
```

`OS-ARCH` uses Rust platform names: `macos-aarch64`, `macos-x86_64`,
`windows-x86_64`, `linux-x86_64`, or `linux-aarch64`. `size` is the exact archive
byte count. `sha256` is 64 lowercase hexadecimal characters. `format` is `tar`,
`tar.gz`, or `zip`. `executable` is the exact relative archive path of Node.
Package archives must not set `executable`. The publisher must increase
`generation` for each changed index. The host refuses an older generation.

The host compares the signed publisher, package, source, version, manifest, and
handshake identity. It stores `owner_binding` as `publisher/package_id`. This
binding survives uninstall. A different publisher or package cannot acquire
that source's saved credentials. The adapter must compare this verified binding
before it reads credentials. Plugin stdout never selects a credential key.

The host caches the signed envelope and persists verified revocations. It merges
revocations across index generations. Discovery failure does not remove usable
installed code or verified revocations. Revocation overrides a version pin.
Verified revocations apply in memory even if local settings cannot be saved.
The host cancels affected work and reports the storage error. An unusable
discovery cache, including a retired signing root, does not block a new trusted
refresh. Corrupt installation state remains a separate actionable failure.

## Source Registry distribution

A Source Registry is a UTF-8 JSON document at an HTTPS URL. OpenKara ships
with no registry. `add_registry(url)` fetches and validates the document before
it stores the URL. Adding a registry installs nothing. `registries()` returns
`{url, registry_id, display_name, homepage, refreshed_at, last_error}` for each
added registry. `refreshed_at` is the Unix time in milliseconds of the last
successful fetch. Two added URLs cannot use the same `registry_id`. A URL that
starts to serve a different `registry_id` keeps its last good listing.

Format version 1 has this shape:

```text
{
  format_version: 1,
  registry_id, display_name, homepage: HTTPS URL,
  packages: [{
    package_id, source_id, name, description, license: SPDX expression,
    versions: [{
      version: SemVer, protocol_majors: [unsigned integer],
      minimum_host_version: SemVer,
      size: byte count, integrity: "sha512-<standard base64>",
      urls: [HTTPS URL],
      resource_origins: [exact HTTPS origin],
      media_origins: [exact HTTPS origin],
      capabilities: [capability]
    }]
  }]
}
```

`registry_id` and `package_id` use lowercase ASCII letters, digits, `.`, `-` and
`_`, start with a letter or digit, and have at most 128 bytes. `source_id` uses
the protocol source id rule. Package ids and versions are unique in their
parent list. `integrity` is the npm Subresource Integrity form with exactly one
SHA-512 digest. A version lists one to eight download URLs and a size of at most
32 MiB. The host refuses the complete document if one field is invalid. The
document is at most 4 MiB.

The host fetches registries over HTTPS only, with the same public-address and
redirect policy as source downloads. `refresh()` fetches every added registry.
The maintenance loop also fetches a registry when its last fetch is six hours
old. A failed fetch keeps the last good listing, stores the error in
`last_error`, and does not fail `refresh()`. Installed sources keep working.
`registry-cache.json` keeps the last good listing of each registry across
restarts.

### Registry packages

`install_package(registry_id, package_id, optional_version)` installs a listed
package. The installed package identity is the pair of `registry_id` and
`package_id`. Its `owner_binding` is `registry:<registry_id>/<package_id>`.
Only one installed package can hold a source id. When another identity holds an
active or staged release for that source id, the install returns
`source_id_taken`. After an uninstall, a different identity can take the source
id. `install(source_id, optional_version)` updates an installed registry
package from its own registry entry.

The host selects the newest listed version whose `protocol_majors` contain 2
and whose `minimum_host_version` is at most the host version. A pin or an
explicit version must match a listed version. If no version qualifies, the host
returns `incompatible_plugin` and downloads nothing. `HostConfig`
`current_protocol` applies only to the Node stdio transport. The embedded
engine and registry packages always use protocol major 2.

The host tries the download URLs in their listed order and stops at the first
URL that serves the exact bytes. It stops a download when the bytes exceed the
declared size, before verification. It checks the byte count and the SHA-512
integrity before it extracts or loads the bytes. When every URL fails, the
error message names each URL that the host tried and its failure reason. The
code is `invalid_package` when a URL served wrong bytes, otherwise `network`.

A package is an npm-style gzip tarball with every member under `package/`. The
package root has `manifest.json` and the manifest `entry` module. The manifest
runtime is `openkara-js` major 1. The manifest must match its registry entry:
source id, package id, version, minimum host version, license, protocol majors,
capabilities, resource origins and media origins. A mismatch returns
`invalid_package`. The host seals the extracted tree in
`packages/<archive-sha256>/`.

A registry candidate passes the engine `describe` compatibility check before it
can become active. Protocol major mismatches return `incompatible_plugin`. A
failed candidate does not replace the active release. A staged update waits for
work leases and requests to finish. The previous package tree stays on disk.

## Installation and runtime

`HostConfig::production(data_root, resource_root, host_version)` creates the
host configuration. `SourceHost::new(config)` reads local state without a network
request or plugin execution. Fresh installation remains local-first.

macOS, Windows, AppImage, and deb use `RuntimePolicy::Downloaded`. The host
verifies the signed runtime archive before it extracts the declared executable
and license notices. It does not install npm, follow archive links, run lifecycle
scripts, or use inherited PATH. Each runtime uses a digest-addressed directory.
Compatible sources share runtime bytes, not a process.

Flatpak uses `RuntimePolicy::ChannelBundled` with
`/app/bin/openkara-source-node`, exact version `24.22.1`, and the native target.
The package work must provision that path. A missing or incompatible executable
requires a package/application update. There is no runtime download fallback.
Both modes execute a bounded version/platform probe and compare exact Node
version, platform, and architecture with the signed release.

### Embedded engine (protocol major 2)

A release with runtime kind `openkara-js` and major 1 runs in the embedded
QuickJS engine, not in a Node process. The signed index lists an empty runtime
version and no runtime targets for this kind. The host reads the manifest
`entry` file as one ES module. It does not download a runtime or start a
process. The module's default export is an object. Each protocol method is a
method of that object with the method name. The host calls the method with the
request params as one JSON argument. The method returns a JSON-compatible
value or a promise. A thrown object with a known protocol error `code` becomes
that error; other throws become `internal`. A missing method returns
`unsupported_operation`. A result is at most 1 MiB of JSON.

Each running source has its own engine runtime, context, and thread. Sources
share no globals. The context has no `eval` and no `Function` constructor
(including the async and generator variants). The host calls `describe` first
with a 10-second deadline. The result must contain protocol major 2 and the
manifest `source_id`, `package_id`, and `package_version`; otherwise the host
returns `incompatible_plugin` and stops the source. Other operations have the
30-second operation deadline. Each source has a 64 MiB memory limit and a
1 MiB script stack limit. When a source passes a deadline or a limit, the host
stops the source and returns `source_unavailable` to all of its pending calls.
The next call starts the source fresh. An embedded source has no crash-restart
backoff.
Package archives contain the protocol manifest, relative `.mjs` entry,
`package-lock.json`, `LICENSE`, `NOTICE`, and locked production dependencies.
The host rejects traversal, absolute paths, links, devices, duplicate paths,
install lifecycle scripts, and excessive expansion. Source archives allow up
to 100,000 members and 4 GiB expanded bytes. The model installer's 512-member
limit does not apply to source dependencies. An individual archive is at most
2 GiB. The installer streams download hashing and file verification.

The data directory is `<data_root>/streaming-sources/`:

- `verified-index.json`: the signed cached envelope.
- `registry-cache.json`: the last good listing of each added Source Registry.
- `state.json`: added registry URLs, owner bindings, registry package
  identities, enablement, pins, active and staged releases, and accumulated
  revocations.
- `packages/<archive-sha256>/`: immutable source trees and verified file records.
- `runtimes/<archive-sha256>/`: immutable managed runtime files.
- `staging/<unique-id>/`: temporary download and extraction state.

A candidate passes byte verification, manifest checks, runtime validation, and
an identity-matched handshake before it can become active. Failed or cancelled
candidates do not replace active code. A successful update waits for all work
leases and requests to finish. The old tree is never overwritten in place.
The host checks revocation again before candidate execution. It checks package
and runtime records after handshake and before delayed activation. Explicit
reinstall repairs a damaged cache in a fresh digest-qualified generation. It
does not overwrite files used by a running process.
Uninstall removes activation and enablement. It retains verified immutable
artifact cache bytes for reinstall; it does not touch library files or credentials.
The application presents the credential retention choice separately.

## Public lifecycle interface

`SourceHost` is cloneable and thread-safe. Its async operations are `refresh`,
`install(source_id, optional_version)`, `retry`, `set_enabled(source_id, enabled,
stop_work)`, and `uninstall(source_id, stop_work)`. `install` is also the manual
update action. Without an explicit version it honors a pin, then selects the
newest compatible non-revoked release. It never performs an automatic update.
A first installation is disabled until the user enables it.
If initialization fails, `SourceHost::unavailable(config, error)` retains that
failure without file writes or processes. The application can keep local
workflows available and report the original source error.

Synchronous operations are `snapshots`, `snapshot`, `description`,
`cancel_install`, `set_pin`, `acquire_work`, and `subscribe`. Pins apply to the
installed version. `description` returns the verified `owner_binding` and manifest.
Snapshots and change subscriptions use:

```text
{
  source_id, owner_binding, name, description, license,
  installed_version: string | null,
  available_version: string | null,
  staged_version: string | null,
  pinned_version: string | null,
  enabled: boolean, runtime_ready: boolean, running: boolean,
  state: not_installed | installing | ready | disabled | update_required |
         unavailable | waiting_for_idle | cancelling,
  progress: {phase, completed, total: integer | null} | null,
  last_error: {code, message, retryable} | null,
  active_work: [{id, kind: authentication | preview | import, cancelling}],
  manifest: SourceManifest | null
}
```

Installation, enablement, runtime readiness, process state, and authentication
are separate. Authentication state comes from the plugin through the adapter,
not from an installation snapshot. Errors contain a stable code, a safe message,
and a retryable flag. They do not contain credentials or upstream response bodies.

A `WorkLease` prevents update activation and idle shutdown. Its cancellation
token tells the consumer to stop source-owned work. The consumer closes windows,
stops download/decoder workers, or completes import cancellation before it drops
the lease. Drop is the acknowledgment. Disable and uninstall without `stop_work`
refuse active work. With approval they signal tokens and wait up to ten seconds
for acknowledgment. A timeout keeps activation and enablement. Revocation signals
leases and rejects new requests immediately, then stops the process at idle.

## Process requests

`request(source_id, method, params)` returns protocol JSON. `start_request`
returns a `RequestHandle` with `id`, `cancellation`, `cancel`,
`subscribe_progress`, and consuming async `result`. Credentials may appear only
in protocol params after the adapter checks owner binding. The host completes
handshake validation before it writes any normal request.

Each source has one managed process. The host supports the current and previous
protocol major, with only major 1 at initial launch. Normal requests are limited
to 16 concurrent operations. Request ids are unique for a process lifetime.
Progress and results stay attached to their host source and request. Unknown ids,
invalid frames, duplicate terminal replies, or lines above 1 MiB stop the process.
Stderr is discarded so plugin diagnostics cannot leak secrets into host logs.

The handshake deadline is ten seconds. Normal requests have a thirty-second
deadline from dispatch, even if the consumer has not awaited their result.
Cancellation sends `request.cancel`. The process has at most two seconds to
settle before termination. A dropped request handle also cancels its request.
The settlement deadline includes cancellation dispatch and stdin backpressure.
Aborted dispatch removes an unsent request or terminates a partial frame.
Idle processes stop after sixty seconds without requests or work leases.
Crashes restart only on the next request. Backoff is one, two, then four seconds;
four failures require explicit Retry or an update. No request is replayed.

## Media and resource boundary

The host does not return a naked validated URL for another client to fetch.
`open_media(source_id, descriptor, optional_range, cancellation)` returns a
`MediaResponse`. It exposes HTTP status, content length, content type, content
range, and async `chunk()`. The preview consumer retains progressive sequential
download, chunk cache, seek, and bounded descriptor refresh. `download_media`
streams to a temporary file and atomically publishes the destination.
`load_resource(source_id, descriptor, cancellation)` limits artwork to 16 MiB.
The consumer supplies its work token. Cancellation interrupts DNS resolution,
connection, redirects, and body reads before the consumer releases its work lease.
`open_media` preserves HTTP status, including 403 and 410. A known expired
RFC 3339 `expires_at` returns `media_expired` before a request. Preview uses
these signals for its existing bounded descriptor refresh. File downloads and
artwork reject unsuccessful responses.

Descriptors contain `url`, `headers`, and optional `expires_at`. The host checks
HTTPS, exact declared media/resource origin, no userinfo, and the case-insensitive
Referer/User-Agent/Cookie header allowlist. It rejects CR/LF and duplicate header
names. Each DNS lookup must return only public destinations. The HTTP client uses
those exact resolved addresses, disables environment proxies and automatic
redirects, and validates each redirect before connecting. This closes the gap
between a safe lookup and a second unvalidated resolver. Cookies are removed on
cross-origin redirects. Host-generated Range never enters the plugin header
allowlist. At most five redirects are followed.

These controls do not sandbox trusted plugin code. A signed plugin can read or
transmit the credentials supplied to it. Signatures establish provenance, not
credential secrecy from authorized code.

## Behavioral evidence

`source_host::tests` uses signed fixture packages, independent generated signing
keys, and a real pinned Node process. Set `OPENKARA_SOURCE_TEST_NODE` to an
explicitly prepared Node 24.22.1 executable and run ignored source-host tests.
The host itself verifies exact runtime version and architecture. The regular
suite also covers local-first failed discovery without Node.

The test-only fixture constructor injects signing keys and an HTTP loopback
_distribution_ endpoint. It cannot change production trust roots. The approved
workflow seam can inject exact public HTTPS resource bytes and DNS answers.
Normal origin, header, and public-address checks still apply to that transport.
No fixture grants production media access to loopback or private addresses.

These development tests are not distributed-app acceptance. Actual source
publication, platform sign-in/media/update acceptance, Flatpak path provisioning,
and first Flathub submission policy remain the named publication and platform
work. Successful runtime feasibility does not establish Flathub approval.
