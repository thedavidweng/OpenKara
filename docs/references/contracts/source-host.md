# Streaming Source Host

The app-independent Rust module `source_host` owns Source Registry discovery,
package installation, and the embedded engine that runs each Streaming Source.
The application adapter owns credentials, authentication windows, preview,
import, and library writes. See
[ADR 0041](../../adr/0041-run-streaming-sources-as-sandboxed-packages-from-user-added-registries.md)
and the [source protocol](streaming-source-protocol.md).

## Build feature

The Cargo feature `streaming-sources` is on by default. A build with
`--no-default-features` compiles Streaming Sources out of the app. That build
does not contain:

- The `source_host` module, the embedded engine, and the engine-only
  dependencies (`rquickjs`, the source crypto crates, `ring`, `semver`, and the
  reqwest cookie store).
- The application adapter: source credentials, authentication windows,
  catalog pages, Streaming Import, and Online Preview.
- Every Streaming Source command in the [catalog contract](catalog.md), except
  `streaming_sources_supported()`.
- The events `streaming-source-status`, `source-registry-link`,
  `streaming-sign-in-complete`, `streaming-sign-in-error`,
  `streaming-import-progress`, and `streaming-import-complete`.
- The `source_registry_link` module and the Tauri deep-link plugin. The config
  overlay `src-tauri/tauri.no-streaming-sources.conf.json` removes the
  `openkara` scheme registration from the bundle.

Build the app without Streaming Sources with
`pnpm tauri build --config src-tauri/tauri.no-streaming-sources.conf.json -- --no-default-features`.
App Store builds always use this build. An iOS build with the feature on
fails to compile. The build does not read or delete
`<app_data>/streaming-sources/` or saved Streaming Credentials, so a later
build with the feature finds them again. Songs that a Streaming Import added
stay ordinary library songs.

## Source Registry distribution

A Source Registry is a UTF-8 JSON document at an HTTPS URL. OpenKara ships
with no registry. `add_registry(url)` fetches and validates the document before
it stores the URL. Adding a registry installs nothing. `registries()` returns
`{url, registry_id, display_name, homepage, refreshed_at, last_error}` for each
added registry. `refreshed_at` is the Unix time in milliseconds of the last
successful fetch. Two added URLs cannot use the same `registry_id`. A URL that
starts to serve a different `registry_id` keeps its last good listing.
`remove_registry(registry_id)` removes an added registry. Its packages leave
the market. Installed packages from it keep working but can no longer update.

`market()` lists every package that the added registries currently offer, one
entry per (registry, package id):

```text
{
  registry_id, package_id, source_id, name, description, license,
  versions: [{
    version, size,
    resource_origins: [exact HTTPS origin],
    media_origins: [exact HTTPS origin],
    capabilities: [capability]
  }],
  installed: {version, pinned_version} | null,
  update: {
    version,
    added_origins: [origin],
    added_capabilities: [capability],
    requires_consent: boolean
  } | null
}
```

`installed` is present when this device has an active release of that exact
package identity. `update` is present when the registry lists a newer
compatible version and no pin holds the installed one. `added_origins` and
`added_capabilities` compare the update's declarations with the installed
manifest; `requires_consent` is true when either is non-empty. An update that
only removes origins or capabilities never needs consent.

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
redirect policy as package downloads. `refresh()` fetches every added registry.
The maintenance loop also fetches a registry when its last fetch is six hours
old. A failed fetch keeps the last good listing, stores the error in
`last_error`, and does not fail `refresh()`. Installed sources keep working.
`registry-cache.json` keeps the last good listing of each registry across
restarts.

### Registry packages

`install_package(registry_id, package_id, optional_version, consent, replace)`
installs a listed package. The installed package identity is the pair of
`registry_id` and `package_id`. Only one installed package can hold a source
id. When another identity holds an active or staged release for that source
id, the install returns `source_id_taken` unless the caller confirms the
replacement with `replace`. A confirmed replacement stops the installed
package's work (refusing while work is active), removes its claim, and installs
the new package disabled, like a first install. Its credentials and key-value
storage do not carry over: both are bound to the owner binding. After an
uninstall, a different identity can take the source id.
`install(source_id, optional_version, consent)` updates an installed registry
package from its own registry entry. A source id that is not installed, or
whose package no added registry lists, returns `not_found`. When an update adds origins or
capabilities compared with the installed manifest, install returns
`consent_required` (naming what was added) until the caller passes
`consent`. Reinstalling the active version never needs consent.

The host selects the newest listed version whose `protocol_majors` contain 2
and whose `minimum_host_version` is at most the host version. A pin or an
explicit version must match a listed version. If no version qualifies, the host
returns `incompatible_plugin` and downloads nothing.

The host tries the download URLs in their listed order and stops at the first
URL that serves the exact bytes. It stops a download when the bytes exceed the
declared size, before verification. It checks the byte count and the SHA-512
integrity before it extracts or loads the bytes. When every URL fails, the
error message names each URL that the host tried and its failure reason. The
code is `invalid_package` when a URL served wrong bytes, otherwise `network`.
Install progress reports phase `package` while the bytes download and phase
`describe` during the compatibility check.

A package is an npm-style gzip tarball with every member under `package/`. The
package root has `manifest.json` and the manifest `entry` module. The manifest
runtime is `openkara-js` major 1. The manifest must match its registry entry:
source id, package id, version, minimum host version, license, protocol majors,
capabilities, resource origins and media origins. A mismatch returns
`invalid_package`. The host accepts only regular files and directories, and
rejects traversal, absolute paths, links, devices, duplicate paths, and more
than 100,000 members or 4 GiB of expanded bytes. The host seals the extracted
tree in `packages/<archive-sha256>/` with a record of every file digest and
checks that record before it loads the entry module.

A registry candidate passes the engine `describe` compatibility check before it
can become active. Protocol major mismatches return `incompatible_plugin`. A
failed or cancelled candidate does not replace the active release. A staged
update waits for work leases and requests to finish. The previous package tree
stays on disk and is never overwritten in place. An explicit reinstall repairs a
damaged package tree in a fresh digest-qualified directory.

### Owner binding

Each installed package has the host-derived `owner_binding`
`registry:<registry_id>/<package_id>`. The binding survives uninstall. A
different package cannot acquire that source's saved credentials or key-value
storage. The adapter stores Streaming Credentials with the owner binding as the
credential account and compares the binding before it reads them. It never
reads a session saved under the plain source id account; clearing a session
removes both accounts. Plugin output never selects a credential key.

### Node-era migration

Earlier releases ran Streaming Sources as Node processes from a signed index.
On every `SourceHost::new`, an installation whose active or staged release
names the Node runtime is marked as needing replacement: the host clears its
active and staged releases, enablement and pin, records the retired name and
version in `state.json`, and deletes `runtimes/`, `verified-index.json`, and
every package tree whose manifest names the Node runtime. A failed deletion is
retried on the next launch. The migration never touches library songs,
Streaming Track Identities or Playlist Origin Stamps, so a replacement under
the same source id keeps matching them.

A retired source keeps its snapshot with state `update_required`, no installed
or available version, its retired name, and `last_error` code
`replacement_required`. `install` and `retry` return `replacement_required`.
The replacement is a registry package installed under the same source id with
`install_package`; it installs disabled, and the old credentials do not carry
over. `replacement_notice()` lists the retired sources as
`{source_id, name, version}` until `dismiss_replacement_notice()`.
`remove_retired_credentials(remove)` calls `remove(source_id, owner_binding)`
once per retired source; a failed removal stays pending for the next call.

## Embedded engine

`HostConfig::production(data_root, resource_root, host_version)` creates the
host configuration. `SourceHost::new(config)` reads local state without a network
request or source execution.

A release with runtime kind `openkara-js` and major 1 runs in the embedded
QuickJS engine inside the host process. The host reads the manifest `entry`
file as one ES module. The module's default export is an object. Each protocol
method is a method of that object with the method name. The host calls the
method with the request params as one JSON argument. The method returns a
JSON-compatible value or a promise. A thrown object with a known protocol error
`code` becomes that error; other throws become `internal`. A missing method
returns `unsupported_operation`. A result is at most 1 MiB of JSON.

Each running source has its own engine runtime, context, and thread. Sources
share no globals. The context has no `eval` and no `Function` constructor
(including the async and generator variants). The host calls `describe` first
with a 10-second deadline. The result must contain protocol major 2 and the
manifest `source_id`, `package_id`, and `package_version`; otherwise the host
returns `incompatible_plugin` and stops the source. Other operations have the
30-second operation deadline. Each source has a 64 MiB memory limit and a
1 MiB script stack limit. When a source passes a deadline or a limit, the host
stops the source and returns `source_unavailable` to all of its pending calls.
The next call starts the source fresh, with no delay. After four consecutive
failed starts the source returns `source_unavailable` until Retry or an update.

### Host API

The [host API reference](streaming-source-host-api.md) is the author-facing
specification of this surface. The global `openkara` object is the only
capability an embedded source gets, together with `setTimeout` and
`clearTimeout`. Binary inputs accept a
`Uint8Array`, an `ArrayBuffer`, or a string (encoded as UTF-8); binary outputs
are `Uint8Array`. Invalid crypto or encoding input throws a `TypeError`.

- `openkara.request({url, method?, headers?, body?, responseType?})` resolves
  to `{status, url, headers, setCookies, body}`. `url` must be HTTPS and its
  exact origin must appear in `resource_origins` or `media_origins`; no
  wildcard, subdomain, port, or userinfo variation is accepted. Methods are
  `GET` (default), `HEAD`, `POST`, `PUT`, `PATCH`, and `DELETE`.
  `responseType` is `text` (default), `json`, or `bytes`. The host follows at
  most 5 redirects and checks the scheme, origin, and public DNS destination at
  every hop; a redirect to an undeclared origin fails. Cross-origin redirects
  drop `Cookie` and `Authorization`. A source may not set `Host`,
  `Content-Length`, `Transfer-Encoding`, `Connection`, `Keep-Alive`, `Upgrade`,
  `TE`, `Trailer`, `Expect`, or any `Proxy-*` or `Sec-*` header; `Referer` and
  `Origin` must be HTTPS URLs without userinfo. Request bodies are at most 1 MiB and
  responses at most 8 MiB. At most 8 requests per source are in flight; more
  reject with `busy`. Rejections carry the host error `code`
  (`invalid_resource`, `network`, `busy`, `cancelled`), so a source that
  rethrows them returns the matching protocol error.
- Each source has its own cookie jar. The host stores `Set-Cookie` responses
  and sends matching cookies on later requests from the same source only;
  explicit `Cookie` headers are sent before stored cookies. `auth.sign_out`
  discards the jar. The jar lives in memory for the running source.
- `openkara.crypto`: `digest(algorithm, data)` and
  `hmac(algorithm, key, data)` with `md5`, `sha1`, `sha256`, `sha384`, or
  `sha512`; `aesEncrypt(options, data)` and `aesDecrypt(options, data)` with
  `options` `{mode, key, iv?, padding?, aad?}`, where `mode` is `ecb`, `cbc`,
  `ctr` (128-bit big-endian counter), or `gcm` (12-byte IV, 16-byte tag
  appended; 128- or 256-bit keys), and `padding` is `pkcs7` (default) or
  `none` for ECB and CBC; `rsaPublic(key, data)` computes the raw RSA public
  operation without padding, with `key` either `{pem}` (`PUBLIC KEY` or
  `RSA PUBLIC KEY`) or `{modulus, exponent}` as big-endian bytes, moduli of
  512 to 8192 bits, and the output left-padded to the modulus length;
  `randomBytes(length)` returns at most 64 KiB of secure random bytes.
- `openkara.encoding`: `base64Encode(data, {urlSafe?, padding?})`,
  `base64Decode(text)` (standard or URL-safe alphabet, padding optional),
  `hexEncode`, `hexDecode`, `utf8Encode`, and `utf8Decode`.
- `openkara.storage`: `get(key)`, `set(key, value)`, `remove(key)`, `clear()`,
  and `keys()` over string keys of 1 to 256 bytes and string values, capped at
  1 MiB of keys and values per source. A write over the cap throws and changes
  nothing. Storage persists in
  `<data_root>/streaming-sources/source-storage/<source_id>.json`, bound to the
  package's owner binding: another package that later takes the same source id
  starts empty. Storage is for source state that is not Streaming Credentials.
- `openkara.log.debug|info|warn|error(message, fields?)` writes a structured
  record to the host log with target `openkara::streaming_source` and the
  source id. Before the record leaves the engine the host replaces credential
  values seen in `session.restore`, sign-in results and `Cookie` or
  `Authorization` request headers, received cookie values, values under
  sensitive field names (for example `token`, `cookie`, `session`, `secret`),
  and URL queries and fragments with `[redacted]`. Messages are cut at 1,024
  characters, fields at 4,096, and each source may write 100 records a minute.
- `setTimeout(callback, delay)` and `clearTimeout(id)` support at most 256
  pending timers per source with delays of at most 60 seconds. Timers run only
  while the source has a call in progress.

`session.restore` receives only the credentials saved for the package's own
owner binding. A source has no API to read the host keychain, another source's
credentials, cookies, or storage.

### Data directory

The data directory is `<data_root>/streaming-sources/`:

- `registry-cache.json`: the last good listing of each added Source Registry.
- `state.json`: added registry URLs, owner bindings, registry package
  identities, enablement, pins, active and staged releases, and retired
  Node-era installs.
- `packages/<archive-sha256>/`: immutable source trees and verified file records.
- `source-storage/<source_id>.json`: each source's key-value storage.
- `staging/<unique-id>/`: temporary download and extraction state.

Uninstall removes activation and enablement. It retains verified immutable
package bytes for reinstall; it does not touch library files or credentials.
The application presents the credential retention choice separately.

## Public lifecycle interface

`SourceHost` is cloneable and thread-safe. Its async operations are `refresh`,
`install(source_id, optional_version, consent)`,
`install_package(registry_id, package_id, optional_version, consent, replace)`,
`add_registry(url)`, `retry`, `set_enabled(source_id, enabled,
stop_work)`, and `uninstall(source_id, stop_work)`. `install` is also the manual
update action. Without an explicit version it honors a pin, then selects the
newest compatible listed version. It never performs an automatic update.
A first installation is disabled until the user enables it.
If initialization fails, `SourceHost::unavailable(config, error)` retains that
failure without file writes or source execution. The application can keep
local workflows available and report the original source error.

Synchronous operations are `snapshots`, `snapshot`, `description`,
`cancel_install`, `set_pin`, `acquire_work`, `remove_registry`, `registries`,
`market`, `replacement_notice`, `dismiss_replacement_notice`,
`remove_retired_credentials`, and `subscribe`. Pins apply to the
installed version. `description` returns the verified `owner_binding` and manifest.
Snapshots and change subscriptions use:

```text
{
  source_id, owner_binding,
  registry_id: string | null, package_id: string | null,
  name, description, license,
  installed_version: string | null,
  available_version: string | null,
  staged_version: string | null,
  pinned_version: string | null,
  enabled: boolean, loaded: boolean, running: boolean,
  state: not_installed | installing | ready | disabled | update_required |
         unavailable | waiting_for_idle | cancelling,
  progress: {phase: package | describe, completed, total: integer | null} | null,
  last_error: {code, message, retryable} | null,
  active_work: [{id, kind: authentication | preview | import, cancelling}],
  manifest: SourceManifest | null
}
```

`registry_id` and `package_id` carry the registry identity of the installed
package and are null for a retired Node-era install. `loaded` reports a
verified active release. Installation, enablement, loading, engine state, and
authentication are separate. Authentication state comes from the source
through the adapter, not from an installation snapshot. Errors contain a stable
code, a safe message, and a retryable flag. They do not contain credentials or
upstream response bodies.

A `WorkLease` prevents update activation and idle shutdown. Its cancellation
token tells the consumer to stop source-owned work. The consumer closes windows,
stops download/decoder workers, or completes import cancellation before it drops
the lease. Drop is the acknowledgment. Disable and uninstall without `stop_work`
refuse active work. With approval they signal tokens and wait up to ten seconds
for acknowledgment. A timeout keeps activation and enablement.

## Source requests

`request(source_id, method, params)` returns protocol JSON. `start_request`
returns a `RequestHandle` with `id`, `cancellation`, `cancel`,
`subscribe_progress`, and consuming async `result`. Credentials may appear only
in protocol params after the adapter checks owner binding. The method names
`handshake` and `request.cancel` are reserved for the host and return
`invalid_request`. The host checks the manifest capability of each operation
before it starts the source.

Each source runs at most 16 concurrent operations; more return `busy`, except
`auth.cancel` and `auth.sign_out`. Progress and results stay attached to their
source and request. Deadlines run from dispatch, even if the consumer has not
awaited the result. Cancellation answers the request with `cancelled` at once;
the cancelled call keeps its deadline until it settles. A dropped request
handle also cancels its request. Idle sources stop after sixty seconds without
requests or work leases. No request is replayed after a stop.

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
cross-origin redirects. Host-generated Range never enters the source header
allowlist. At most five redirects are followed.

The engine sandbox limits what a source can reach, not what it does with the
credentials supplied to it: a source can transmit its own credentials to its
declared origins. Registry integrity establishes that the bytes match the
listing, not that the publisher is trustworthy.

## Behavioral evidence

The host tests need no Node runtime and no network. They install packages from
a mock Source Registry (`source_host::test_support::RegistryFixture`) over
loopback HTTP, which only the test configuration allows. `engine_tests`,
`host_api_tests`, `registry_tests`, `media_tests`, and `migration_tests` cover
the engine limits, the host API, registry installation, media policy, and the
Node-era migration. The test configuration can inject exact public HTTPS
resource bytes and DNS answers, and can route a declared origin to a local mock
server. Normal origin, header, and public-address checks still apply to that
transport. No fixture grants production media access to loopback or private
addresses.

These development tests are not distributed-app acceptance. Actual source
publication and platform sign-in, media, and update acceptance remain the named
publication and platform work.
