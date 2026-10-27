# Streaming Source protocol 2.0

Status: implemented host contract for protocol major 2 on the `openkara-js`
runtime. See the [source host](source-host.md) for registry distribution,
installation, lifecycle, and media boundaries, and the
[host API reference](streaming-source-host-api.md) for the sandboxed
`openkara` object. License: AGPL-3.0-only; see the
[rights inventory](../streaming-source-provenance.md). Architecture:
[ADR 0041](../../adr/0041-run-streaming-sources-as-sandboxed-packages-from-user-added-registries.md).

Protocol 2.0 keeps the methods, data shapes, capabilities, authentication
flows, cursors, and error codes of protocol 1.0. The runtime and the
transport changed: a protocol 2.0 package is one ES module that the host
runs in an embedded QuickJS engine instead of a Node stdio process. The
[changes from protocol 1.0](#changes-from-protocol-10) section lists every
difference. The application no longer runs protocol 1.0 packages and has no
stdio transport: it marks Node-based installations as needing replacement
([Node-era migration](source-host.md#node-era-migration)). New packages use
protocol major 2.

## Runtime and transport

The package entry is one self-contained ES module. The host reads the
verified entry file and loads it as a module in the embedded engine. No
process starts. The module has no Node APIs, no filesystem, no sockets, and
no module loader: a static or dynamic `import` fails. The sandbox gives the
module the global `openkara` object plus `setTimeout` and `clearTimeout`,
and nothing else. The [host API reference](streaming-source-host-api.md)
specifies them.

The module's default export is an object. Each protocol method is a function
property of that object under the method name. The host calls the function
with the request params as one JSON argument and the exports object as
`this`. The function returns a JSON-compatible value or a promise of one.
A result of `undefined` becomes JSON null. A result serializes to at most
1 MiB of JSON; a larger result fails the call as `internal`.

A thrown value with a string `code` from the [error list](#errors) becomes
that protocol error. The host replaces its message with its own sanitized
text; a message from a source never reaches the user. Any other throw
becomes `internal`. A missing method becomes `unsupported_operation`.

Each running source has its own engine runtime, context, and thread.
Sources share no globals. The context has no `eval` and no `Function`
constructor, including the async and generator variants. It has the `Date`,
`RegExp`, `JSON`, `Proxy`, `Map`/`Set`, typed-array, `Promise`, and
`WeakRef` intrinsics.

The host calls `describe` with a 10-second deadline and every other method
with a 30-second deadline. Each source has a 64 MiB memory limit and a
1 MiB script stack limit. At most 16 calls per source run at once; more
fail as `busy` before dispatch. When a source passes a deadline or a limit,
the host stops the source and fails all of its pending calls as
`source_unavailable`. The next call starts the source fresh. An embedded
source has no crash-restart backoff. The host stops an idle source after
60 seconds without calls or active work. The source's in-memory state,
pending timers, and cookie jar end with the engine instance.

## Compatibility and describe

Protocol 2.0 has no handshake request. When the host starts a source, it
first calls `describe` with an offer:

```json
{ "protocol_majors": [2], "host_version": "0.18.1" }
```

The result must contain `protocol.major` 2 and the manifest's `source_id`,
`package_id`, and `version` (as `package_version`). A source that does not
support an offered major throws `incompatible_plugin`. A result whose
identity or version fields do not match the verified manifest is also
`incompatible_plugin`. The host stops the source in both cases.
Installation runs the same describe check on a candidate before activation,
so an incompatible update never replaces the active release.

Other `describe` calls can carry `{}` with no offer. A source must tolerate
both forms. One engine run never negotiates twice.

`incompatible_plugin` also covers selection-time mismatches. A registry
install selects the newest listed version whose `protocol_majors` contain 2
and whose `minimum_host_version` is at most the host version. When no
listed version qualifies, the install fails as `incompatible_plugin` and
the host downloads nothing. An unknown manifest runtime kind fails the
same way. A protocol minor change is additive; new required fields or
changed meanings require a new major.

## Package and identity

A registry package is an npm-style gzip tarball. Every member sits under a
single `package/` directory. That directory holds `manifest.json` and the
manifest's `entry` module, and can hold `LICENSE`, `NOTICE`, and other data
files. The host rejects absolute paths, traversal, links, device files,
duplicate entries, and install lifecycle scripts. It never runs package
scripts. A listed version is at most 32 MiB of archive bytes.

`manifest_version` is 1. Required manifest fields:

| Field                                     | Meaning                                                                                                                          |
| ----------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `package_id`, `publisher`, `source_id`    | Stable package, publisher, and source identity. The source id is lowercase ASCII `[a-z][a-z0-9_-]{0,63}`.                        |
| `version`, `minimum_host_version`         | SemVer strings. The host uses SemVer ordering.                                                                                   |
| `protocol`                                | `{ "majors": [2], "minor": 0 }`. The majors list is nonempty.                                                                    |
| `runtime`                                 | `{ "kind": "openkara-js", "major": 1 }` for the embedded engine.                                                                 |
| `entry`, `name`, `description`, `license` | Relative `.mjs` entry, plain display text, SPDX expression. No HTML or executable presentation.                                  |
| `capabilities`                            | Known capability strings listed below.                                                                                           |
| `authentication`                          | Authentication declaration below.                                                                                                |
| `requires_authentication`                 | Method names that require a verified session.                                                                                    |
| `resource_origins`, `media_origins`       | Exact HTTPS origins that `openkara.request` and host media/resource loads may reach. No wildcard, subdomain, or port variations. |

The manifest must match its registry entry: source id, package id, version,
minimum host version, license, protocol majors, capabilities, resource
origins, and media origins. A mismatch fails the install as
`invalid_package`.

The installed package identity is the pair of registry id and package id.
Its owner binding is `registry:<registry_id>/<package_id>`. Streaming
Credentials and key-value storage bind to it: a different package that
later takes the same source id cannot read them. Only one installed package
can hold a source id at a time; see [lifecycle](#lifecycle).

The host attaches verified source identity to all results, track
references, playlist stamps, preview keys, cursors, and request state.
Remote ids are opaque strings, not globally unique integers. A source
cannot supply library ids, local paths, or another source's ids.

## Description, capabilities, and sessions

Methods that every source implements: `describe`, `session.restore`,
`auth.start`, `auth.poll`, `auth.complete`, `auth.cancel`, `auth.sign_out`.
Auth methods that do not apply to the declared flow throw
`unsupported_operation`. `describe` returns the manifest's `name`,
`description`, `capabilities`, `authentication`, and
`requires_authentication`, the `protocol` and identity fields from the
compatibility section, and `session`.

`Session` is `{ "state": "signed_out" | "signed_in" | "expired",
"account": null | { "id": "opaque", "display_name": "plain text" } }`.
An expired session is never an empty successful result. A signed-out source
can serve public operations; authentication is not a universal gate.

| Capability        | Method            | Required params                         | Result                                          |
| ----------------- | ----------------- | --------------------------------------- | ----------------------------------------------- |
| `search`          | `search`          | `query: string, cursor: string or null` | `Page<Track>`                                   |
| `liked_tracks`    | `liked_tracks`    | `cursor`                                | `Page<Track>`                                   |
| `playlists`       | `playlists`       | `cursor`                                | `Page<StreamingPlaylist>`                       |
| `playlist_tracks` | `playlist.tracks` | `playlist_id: string, cursor`           | `{playlist: StreamingPlaylist, ...Page<Track>}` |
| `track_details`   | `tracks.get`      | `track_ids: string[]` (1–100)           | `{items: Track[], missing_ids: string[]}`       |
| `preview`         | `media.resolve`   | `track_id: string, purpose: "preview"`  | `Resolution`                                    |
| `import`          | `media.resolve`   | `track_id: string, purpose: "import"`   | `Resolution`                                    |

A method or purpose without its capability returns
`unsupported_operation`, even when the session is signed out. Hosts show
only known declared actions; they ignore unknown additive capabilities.
Known capability semantics cannot change in a minor version. A playlist
browser needs both `playlists` and `playlist_tracks`; `import` does not
require `preview`.

`Page<T>` is `{ "items": T[], "next_cursor": string | null }`. Null ends
the list. There is no total, offset, full id list, fixed page size, or
guarantee of a full page. A non-null cursor means more data even on an
empty page. Cursors bind to the operation, the query or playlist, and the
session. They can expire or be single-use; return `invalid_cursor` and let
the user restart the list. Hosts never decode, edit, or reuse cursors
across sources, queries, or sessions. Sources own batching, upstream
scheduling, snapshot semantics, and normalization. A playlist page carries
its summary each time; the host need not fetch every id in advance.

## Authentication

Credentials are a string-to-string map, opaque to the host. Only verified
success supplies credentials for persistence. Do not log them or include
them in progress. `session.restore {credentials}` receives only the
credentials saved for the package's own owner binding. It verifies them
with the service and returns `Session`. Failure throws `session_expired`
and sets the session expired. It does not invent a signed-in display name
from storage. `auth.sign_out {}` clears the session and open challenges and
returns a signed-out Session. The host also discards the source's cookie
jar, deletes its saved credentials, and closes auth windows even when the
engine is unavailable. Disable retains credentials; sign-out does not.

### Official web page

Manifest authentication:
`{"flow":"web","sign_in_url":"https://service.example/login",
"navigation_origins":["https://service.example"],"cookie_names":["SESSION"]}`.
The sign-in URL's origin must appear in `navigation_origins`, and
`cookie_names` must be a nonempty list of valid cookie names. The host
opens an isolated window without app IPC and allows only the declared
origins. It captures only the listed cookies. `auth.start {}` returns
`{challenge, expires_at, presentation:{kind:"web"}}`.
`auth.complete {challenge, credentials}` verifies the account and returns
`{session, credentials}` only on success. An unknown or expired challenge
throws `challenge_expired`. Window closure calls `auth.cancel {challenge}`.
Closing or cancelling cannot persist partial credentials. `auth.poll` does
not apply to this flow and throws `unsupported_operation`.

### Source-driven prompt

Manifest authentication: `{"flow":"plugin","presentations":["qr","text"]}`
or a nonempty subset. `auth.start {}` returns `{challenge, expires_at,
poll_after_ms, presentation}`. Presentation is either `{kind:"qr",
content:"opaque text to encode as QR", text:"plain instructions"}` or
`{kind:"text", text:"plain instructions"}`. The host generates the QR image
from the content and renders the text as text, never as markup, script, or
an automatic navigation. `auth.poll {challenge}` returns one of:

- `{state:"pending", poll_after_ms}`;
- `{state:"complete", session, credentials}` after verification;
- `{state:"expired"}`;
- `{state:"cancelled"}`.

Only `complete` can contain credentials. `auth.cancel {challenge}` returns
`{state:"cancelled"}` and invalidates that challenge. It is idempotent for
a known challenge. Polling a cancelled or expired known challenge preserves
its terminal state. Unknown challenges throw `challenge_expired`. Sign-out
and a new sign-in invalidate earlier challenges. `auth.complete` does not
apply to this flow and throws `unsupported_operation`. All timestamps use
RFC 3339 UTC; poll intervals are positive integer milliseconds.
`authentication: {"flow":"none"}` is valid for public sources: its auth
calls are unsupported and it declares no auth-required methods.

## Domain data and resources

All strings are plain data. Required nullable fields use null, not
omission. Unknown optional metadata can be omitted. Duration is
milliseconds, sizes are bytes, bit rate is bits per second. Integers are
JSON-safe nonnegative integers.

`Track` is `{id, title, artists: string[], album: string | null,
duration_ms: number | null, artwork: Resource | null,
refusal: ImportRefusal | null}`. Id and title are nonempty strings.
`StreamingPlaylist` is `{id, name, description: string | null,
artwork: Resource | null, track_count: number | null}`.

`Resource` is `{url: "https://...", headers: {}, expires_at: string | null}`.
Use directly usable artwork URLs. CDN resizing belongs to the source. The
host loads resources outside the main-window CSP and exposes managed
resource handles to the UI.

`Resolution` is exactly one of:

- `{kind:"media", media:{url, headers, expires_at, content_type,
  file_extension, size_bytes, duration_ms, bit_rate}}`;
- `{kind:"refusal", refusal:ImportRefusal}`.

The three numeric metadata fields and `expires_at` can be null.
`content_type` and `file_extension` describe the full audio (for example
`audio/mpeg` and `mp3`); the extension has no leading dot or path
separator. Never use audio bytes, local file paths, or an upstream API
endpoint as media. `ImportRefusal` is `{reason:"no_play_rights" |
"trial_clip" | "no_media" | "unavailable", message:"plain explanation"}`.
Refusals are domain results, not errors. A trial clip is refused for both
preview and import. The host does not fall back to another source, even
for a matching title.

The host validates the HTTPS scheme, the exact declared origin, the DNS/IP
destination, and each redirect before a media or resource download. It
rejects userinfo, local-file schemes, loopback, private and link-local
networks, and redirects outside the declared origins. Descriptor headers
are limited to `Referer`, `User-Agent`, and `Cookie`, with CR/LF and
duplicates rejected. Cookies are stripped on cross-origin redirects. The
host handles downloads and temporary files, and re-resolves expired URLs
through the bounded preview refresh path. A resolution never writes a
library row. Import uses the one shared host job and the normal Library
Decisions.

## Cancellation and errors

The host owns cancellation. Protocol 2.0 has no `request.cancel` method;
the method names `handshake` and `request.cancel` belong to the host and
fail as `invalid_request`. When the user or the host cancels a call, the
call fails as `cancelled`. The host does not interrupt module code that is
still running. A cancelled call keeps its 30-second deadline until it
settles; a call that never settles stops the source when the deadline
passes. Keep operations inside the deadline and start no more work than a
call needs.

The embedded engine has no progress-event API. The progress envelope of
protocol 1.0 belonged to the stdio transport. A long operation stays
inside one call and its deadline.

### Errors

| Error code                | Meaning / host action                                                                      |
| ------------------------- | ------------------------------------------------------------------------------------------ |
| `invalid_request`         | Bad params; do not retry unchanged.                                                        |
| `incompatible_plugin`     | No compatible protocol, or identity mismatch; update required.                             |
| `unsupported_operation`   | Missing capability or wrong auth flow; the host removes the unavailable action.            |
| `authentication_required` | Sign in for this operation.                                                                |
| `session_expired`         | The source rejected saved or current credentials; show sign-in, not an empty list.         |
| `challenge_expired`       | Restart sign-in.                                                                           |
| `invalid_cursor`          | Restart the relevant list, not another source's list.                                      |
| `not_found`               | Unknown track or playlist.                                                                 |
| `network`                 | Recoverable upstream connection failure.                                                   |
| `rate_limited`            | Upstream rate limit; the user can retry later.                                             |
| `busy`                    | Concurrency bound reached; the user can retry.                                             |
| `cancelled`               | Work cancelled; no success side effects.                                                   |
| `source_disabled`         | Host rejected dispatch to a disabled source; never sent to the source.                     |
| `source_unavailable`      | The host stopped the engine, or it hit a deadline or limit; no fallback to another source. |
| `internal`                | Sanitized source failure; never exposes upstream credential-bearing data.                  |

Every error carries `code`, a plain `message`, and a boolean `retryable`.
The host sets the message and the flag: a source that throws a known code
controls the code only. `network`, `busy`, and `source_unavailable` are
retryable; the other codes are not. The protocol 1.0 `retry_after_ms` hint
has no transport in the embedded engine.

## Lifecycle

Installation, update, and uninstall are host workflows that the
[source host](source-host.md) specifies. The rules a package author
depends on:

- Install and update download the listed archive, stop the download when
  the bytes exceed the declared size, and verify the byte count and the
  SHA-512 integrity before extraction. A candidate must also match its
  registry entry and pass the describe compatibility check before it can
  become active. A failed candidate never replaces the active release.
- A first installation stays disabled until the user enables it.
- An update that adds origins or capabilities, compared with the installed
  manifest, requires explicit user consent. A removal-only update or a
  same-version reinstall never does.
- When another package identity holds the source id, the install fails as
  `source_id_taken` until the user confirms the replacement. A confirmed
  replacement stops the old package's work, removes its claim, and installs
  the new package disabled. Credentials and key-value storage do not carry
  over.
- A package that replaces a Node-era installation must keep its source id so
  that library songs and playlists keep matching. The host removed the old
  credentials, so the user signs in again.
- Uninstall removes activation and enablement. It retains the verified
  package bytes and the source's key-value storage file. It does not touch
  library files or credentials; the application offers the credential
  retention choice separately. A different package that later takes the
  source id starts with empty storage.

## Publishing through a Source Registry

A Source Registry is a UTF-8 JSON document at an HTTPS URL, maintained
outside OpenKara. OpenKara ships with no registry. The user adds registries
by URL. The [source host](source-host.md#source-registry-distribution)
contract gives the full format; the author-facing summary:

```text
{
  format_version: 1,
  registry_id, display_name, homepage: HTTPS URL,
  packages: [{
    package_id, source_id, name, description, license: SPDX expression,
    versions: [{
      version: SemVer, protocol_majors: [2],
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

- `registry_id` and `package_id` use lowercase ASCII letters, digits, `.`,
  `-`, and `_`, start with a letter or digit, and have at most 128 bytes.
  The `source_id` follows the protocol source id rule. Package ids and
  versions are unique in their parent list.
- `integrity` is the npm Subresource Integrity form with exactly one
  SHA-512 digest of the archive bytes. A version lists one to eight
  download URLs and a nonzero size of at most 32 MiB. The host tries the
  URLs in their listed order and stops at the first URL that serves the
  exact bytes. When every URL fails, the error names each URL and its
  failure reason.
- A registry document is at most 4 MiB, with at most 1000 packages and at
  most 200 versions per package. The host refuses the complete document
  when one field is invalid, and keeps the last good listing on fetch
  failures.
- The registry id is the registry's identity, not its URL. Two added URLs
  cannot use the same `registry_id`, and a second URL that claims an
  already-known id is refused. A legitimate move works through remove and
  re-add.
- A registry entry and its package manifest must agree; sign nothing. The
  integrity declaration, the origin allowlist, and the sandbox are the
  security boundary.
- A registry website can offer a one-click add link:
  `openkara://add-registry?url=<percent-encoded registry URL>`. The link
  only opens OpenKara's add confirmation, which names the URL and states
  that the registry is maintained outside OpenKara; nothing is added until
  the user confirms. See the [catalog contract](catalog.md#source-registry-links)
  for the accepted form.

The [deterministic fixture package](../../../fixtures/streaming-source/README.md)
is a complete working protocol 2.0 package: one ES module with colliding
test ids, single-use cursors, QR and text sign-in, a trial-clip refusal,
and media resolution through `openkara.request`.

## Changes from protocol 1.0

- Transport: the Node stdio NDJSON framing, its request ids, envelopes,
  frame size limit, and process deadlines are gone. Method dispatch is a
  function call on the module's default export.
- Negotiation: the `handshake` method is gone. The startup `describe`
  carries the protocol offer, and identity mismatches fail as
  `incompatible_plugin` the same way.
- Control methods: `request.cancel` is gone. Cancellation is host-side and
  fails the call as `cancelled`.
- Progress: the progress event envelope has no engine transport.
- Errors: the host owns error messages and the retryable flag. The
  `retry_after_ms` hint is not transported. `handshake_required` cannot
  occur.
- Runtime: the manifest runtime is `openkara-js` major 1. The host
  downloads no runtime and starts no process. The engine limits (memory,
  stack, deadlines, concurrency) replace process supervision; a stopped
  embedded source restarts fresh with no backoff.
- Distribution: user-added Source Registries with SHA-512 integrity replace
  the signed index, signing keys, and revocations. The owner binding is
  `registry:<registry_id>/<package_id>`.
- Storage: protocol 2.0 adds the per-package key-value store. The cookie
  jar is in-memory per running source; packages that need durable cookies
  store them through `session.restore` credentials or the key-value store.
