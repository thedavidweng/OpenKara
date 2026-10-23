# Streaming Source protocol 1.0

Status: implemented host contract. See the [source host](source-host.md) for
signed distribution, lifecycle, and download interfaces. License: AGPL-3.0-only;
see [rights inventory](../streaming-source-provenance.md).
Architecture: [ADR 0039](../../adr/0039-install-streaming-sources-as-independent-trusted-packages.md).
The existing bundled NetEase wire format is not protocol 1.0.

## Transport and negotiation

One UTF-8 JSON object per LF-terminated line on stdin/stdout. CRLF is accepted.
No BOM, blank lines, batches, binary payloads, or stdout diagnostics. Stderr
must not contain credentials, challenges, cookies, or signed media URLs. Each
line is at most 1 MiB including LF. Malformed frames, reused request ids, and
invalid envelopes are fatal protocol violations: stop the process, fail pending
requests as `source_unavailable`, and do not retry that request automatically.
EOF cancels pending work and exits. Writers honor stream backpressure.

Host request: `{"id":"h1","method":"describe","params":{}}`.
Ids are nonempty strings of at most 128 characters, unique for the process
lifetime. Results can arrive out of order. Every accepted request has exactly
one terminal response, either `{"id":"h1","result":{...}}` or
`{"id":"h1","error":{"code":"network","message":"Source request failed","retryable":true}}`.
A response must not contain both fields. Notifications use the event envelope
below, not a response id. Unknown additive object fields are ignored. Unknown
enum variants are not silently treated as success. Reject invalid required
fields with `invalid_request`. Never put raw upstream bodies in errors.

The first request is `handshake` with only
`{"protocol_majors":[1],"host_version":"0.17.0"}`. The host sends no credentials
and accepts no normal results before this succeeds. A plugin selects the highest
common major and returns:

```json
{
  "id": "h1",
  "result": {
    "protocol": { "major": 1, "minor": 0 },
    "package_id": "org.openkara.fixture",
    "source_id": "fixture",
    "package_version": "1.0.0"
  }
}
```

The host compares all identity/version fields with the verified manifest and
index before accepting the process. A mismatch is `incompatible_plugin` and
terminates it. No common major returns `incompatible_plugin`; the host then
terminates it without credentials. A second handshake is `invalid_request`.
Before handshake, other methods return `handshake_required` without changing
session state. Protocol minor changes are additive; new required fields or
changed meanings require a new major. The host supports its current major and
immediately previous major (only 1 at initial launch). Package SemVer is
independent. Check compatibility before startup when metadata is sufficient.
Outside-window packages show update-required without losing enablement or
credentials. Pinned packages follow the same rule.

The host allows at most 16 concurrent normal requests per process. Plugins
return `busy` above their limit; cancellation/control requests remain available.
The host applies a 10-second handshake and 30-second operation deadline.
Long-lived sign-in uses short polling requests, not one unlimited request.
On timeout send cancellation, wait at most 2 seconds, then terminate a process
that does not settle. Startup crashes use bounded backoff (1, 2, 4 seconds,
then unavailable until explicit retry). Restart on next use after a normal
crash, not on a hidden infinite loop. Stop after 60 idle seconds with no active
sign-in, preview, or import. Explicit stop cancels work before termination.

## Package and identity

Archive root contains `manifest.json`, a relative `.mjs` entry, lockfile,
production dependency tree (if any), attribution and license material. No path
may escape the archive root; reject absolute paths, traversal, symlinks, device
files, duplicate entries, and install scripts. The client never installs npm
dependencies. `manifest_version` is 1. Required manifest fields:

| Field                                     | Meaning                                                                                                   |
| ----------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `package_id`, `publisher`, `source_id`    | Stable verified package/publisher/source binding. Lowercase ASCII source id `[a-z][a-z0-9_-]{0,63}`.      |
| `version`, `minimum_host_version`         | SemVer strings; host uses SemVer ordering.                                                                |
| `protocol`                                | `{ "majors": [1], "minor": 0 }`; nonempty supported majors.                                               |
| `runtime`                                 | `{ "kind": "node", "major": 24 }`; exact version and platform bytes come from the signed index, not PATH. |
| `entry`, `name`, `description`, `license` | Relative entry, plain display text, SPDX expression. No HTML or executable presentation.                  |
| `capabilities`                            | Known capability strings listed below.                                                                    |
| `authentication`                          | Auth declaration below.                                                                                   |
| `requires_authentication`                 | Array of method names that require a verified session.                                                    |
| `resource_origins`, `media_origins`       | Exact HTTPS origins allowed for resource/media requests, including scoped cookies. No wildcard origins.   |

The signed index uses a dedicated signing key. It binds these identities,
versions, archive HTTPS URL, byte size, SHA-256, protocol, host requirement,
exact runtime version, runtime per-target URLs/sizes/SHA-256, and revocations.
Signature verification precedes parsing trusted release data. Key rotation uses
embedded current/next public keys. Package ids supplied on stdout never select
credentials. The host attaches verified source identity to all results/events,
track references, playlist stamps, preview keys, cursors, and request/job state.
Remote ids are opaque strings, not globally unique integers. No request contains
an arbitrary source id for dispatch to a different process.

The host chooses runtime delivery from its application channel, not plugin
output. macOS, Windows, AppImage, and deb install verified runtime bytes from
the signed index. Flatpak uses the pinned executable in its application
package and never downloads a runtime executable. Both modes validate exact
version, target, and protocol/host compatibility against the signed release.
If the bundled runtime cannot satisfy a release, show update-required and
retain the usable installation; do not fall back to an executable download.

## Description, capabilities, and sessions

Mandatory methods: `describe`, `session.restore`, `auth.start`, `auth.poll`,
`auth.complete`, `auth.cancel`, `auth.sign_out`, `request.cancel`.
Auth methods that do not apply to the declared flow return
`unsupported_operation`. `describe` takes `{}` and returns the manifest's
`name`, `description`, `capabilities`, `authentication`,
`requires_authentication`, plus `session`.

`Session` is `{ "state": "signed_out" | "signed_in" | "expired",
"account": null | { "id": "opaque", "display_name": "plain text" } }`.
An expired session is never an empty successful result. A signed-out source can
serve public operations; authentication is not a universal browser gate.

| Capability        | Method            | Required params                         | Result                                          |
| ----------------- | ----------------- | --------------------------------------- | ----------------------------------------------- |
| `search`          | `search`          | `query: string, cursor: string or null` | `Page<Track>`                                   |
| `liked_tracks`    | `liked_tracks`    | `cursor`                                | `Page<Track>`                                   |
| `playlists`       | `playlists`       | `cursor`                                | `Page<StreamingPlaylist>`                       |
| `playlist_tracks` | `playlist.tracks` | `playlist_id: string, cursor`           | `{playlist: StreamingPlaylist, ...Page<Track>}` |
| `track_details`   | `tracks.get`      | `track_ids: string[]` (1–100)           | `{items: Track[], missing_ids: string[]}`       |
| `preview`         | `media.resolve`   | `track_id: string, purpose: "preview"`  | `Resolution`                                    |
| `import`          | `media.resolve`   | `track_id: string, purpose: "import"`   | `Resolution`                                    |

A method/purpose without its capability returns `unsupported_operation`, even
if the session is signed out. Hosts show only known declared actions; ignore
unknown additive capabilities rather than inventing UI. Known capability
semantics cannot change in a minor version. A playlist browser needs both
`playlists` and `playlist_tracks`; `import` does not require `preview`.

`Page<T>` is `{ "items": T[], "next_cursor": string | null }`.
Null ends the list. No total, offset, full id list, fixed page size, or guarantee
of a full page. A non-null cursor means more data even on an empty page.
Cursors bind to operation, query/playlist, and session. They can expire; return
`invalid_cursor` and let the user restart the list. Hosts never decode, edit,
or reuse them across sources, queries, or sessions. Plugins own batching,
upstream scheduling, snapshot semantics, and normalization. A playlist page
carries its summary each time; the host need not fetch every id in advance.

## Authentication

Credentials are a string-to-string map, opaque to the host. Only verified
success supplies credentials for persistence. Do not log them or include them
in progress. `session.restore {credentials}` verifies saved credentials with
the source and returns `Session`; failure returns `session_expired` and sets
session expired. It does not invent a signed-in display name from storage.
`auth.sign_out {}` clears process session/challenges and returns signed-out
Session. The host deletes its saved credentials and closes auth windows even
if the process is unavailable. Disable retains credentials; sign-out does not.

### Official web page

Manifest authentication:
`{"flow":"web","sign_in_url":"https://music.163.com/#/login",
"navigation_origins":["https://music.163.com"],"cookie_names":["MUSIC_U","__csrf"]}`.
Final NetEase publication must verify the needed official origins on each
platform; no wildcard expansion is implied here. The host opens an isolated
window without app IPC and allows only declared origins. It captures only
listed cookies. `auth.start {}` returns
`{challenge, expires_at, presentation:{kind:"web"}}`.
`auth.complete {challenge, credentials}` verifies the account and returns
`{session, credentials}` only on success. Unknown/expired challenges return
`challenge_expired`. Window closure calls `auth.cancel {challenge}`. Closing or
cancelling cannot persist partial credentials. NetEase cookie keys and upstream
verification details remain in the package, not host brand branches.

### Plugin-driven prompt

Manifest authentication: `{"flow":"plugin","presentations":["qr","text"]}`
(or a nonempty subset). `auth.start {}` returns
`{challenge, expires_at, poll_after_ms, presentation}`. Presentation is either
`{kind:"qr", content:"opaque text to encode as QR", text:"plain instructions"}`
or `{kind:"text", text:"plain instructions"}`. The host generates the QR image
from content and renders text as text, never markup, script, or an automatic
navigation. This avoids arbitrary SVG or remote image execution.
`auth.poll {challenge}` returns one of:

- `{state:"pending", poll_after_ms}`;
- `{state:"complete", session, credentials}` after verification;
- `{state:"expired"}`;
- `{state:"cancelled"}`.

Only complete may contain credentials. `auth.cancel {challenge}` returns
`{state:"cancelled"}` and invalidates that challenge. It is idempotent for a
known challenge. Polling a cancelled or expired known challenge preserves its
terminal state. Unknown challenges return `challenge_expired`. Sign-out and
new sign-in invalidate earlier challenges. Polling is not progress on another
source or request. All timestamps use RFC 3339 UTC; poll intervals are positive
integer milliseconds. `authentication:{flow:"none"}` is valid for public sources;
its auth calls are unsupported and it has no auth-required methods.

## Domain data and resources

All strings are plain data. Required nullable fields use null, not omission.
Unknown optional metadata can be omitted. Duration is milliseconds, sizes are
bytes, bit rate is bits/second. Integers must be JSON-safe nonnegative integers.

`Track` is `{id, title, artists: string[], album: string | null,
duration_ms: number | null, artwork: Resource | null,
refusal: ImportRefusal | null}`. Id/title are nonempty strings. `StreamingPlaylist`
is `{id, name, description: string | null, artwork: Resource | null,
track_count: number | null}`. The host supplies source identity; plugins cannot
supply library ids, local paths, or another source's ids.

`Resource` is `{url: "https://...", headers: {}, expires_at: string | null}`.
Use directly usable artwork URLs. CDN resizing belongs to the plugin. The
host loads resources outside the main-window CSP and exposes managed resource
handles to UI. Do not add each source's CDN to the main-window CSP.

`Resolution` is exactly one of:

- `{kind:"media", media:{url, headers, expires_at, content_type,
  file_extension, size_bytes, duration_ms, bit_rate}}`;
- `{kind:"refusal", refusal:ImportRefusal}`.

The three numeric metadata fields and `expires_at` may be null.
`content_type` and `file_extension` describe the full audio (for example
`audio/mpeg` and `mp3`); extension has no leading dot or path separator.
Never use audio bytes, local file paths, or an upstream API endpoint as media.
`ImportRefusal` is `{reason:"no_play_rights" | "trial_clip" | "no_media" |
"unavailable", message:"plain explanation"}`. Refusals are domain results,
not transport errors. A trial clip is refused for both preview and import.
The host must not fall back to another source, even for a matching title.

The host validates HTTPS scheme, origin, DNS/IP destination and each redirect.
Reject userinfo, local-file schemes, loopback, private/link-local networks, and
redirects outside the declared origins before a download. Recheck each hop to
avoid redirect/hostname bypass. Host header allowlist is case-insensitive
`Referer`, `User-Agent`, `Cookie` only; reject all others, CR/LF values, and
Cookie outside the relevant declared origins. Strip cookies on cross-origin
redirects. This does not sandbox a trusted plugin's own network. Production
hosts do not accept private media merely because a fixture uses a test server.
The host handles downloads and temporary files, and re-resolves expired URLs
through the existing bounded preview refresh path. Resolution never writes a
library row. Import uses the one shared host job and normal Library Decisions.

## Cancellation, progress, and errors

`request.cancel {request_id}` returns `{cancelled:true}` if the target is still
pending, otherwise `{cancelled:false}`. A true cancellation gives the target
exactly one terminal `cancelled` error, no later result or progress. False means
the terminal response already won the race. Cancel requests are never targets.
Cancelling one poll does not cancel the sign-in challenge; use `auth.cancel`.
The host discards late events after removal or request-generation changes.

Progress is optional for long operations, before their terminal response:
`{"event":"progress","request_id":"h7","data":{"phase":"resolve",
"completed":1,"total":3}}`. `phase` is plain text, `completed` is a nonnegative
integer, and `total` is null or an integer not below completed. It describes
source work, not host download/import completion. No unsolicited credentials,
source-id routing, library mutation, or job ids originate in a plugin. The host
binds progress to its own source/request/import job and keeps the job active
when the user changes browser pages.

| Error code                | Meaning / host action                                                    |
| ------------------------- | ------------------------------------------------------------------------ |
| `invalid_request`         | Bad params; do not retry unchanged.                                      |
| `handshake_required`      | Credentials/normal operation sent too early; host defect.                |
| `incompatible_plugin`     | No compatible protocol or identity mismatch; update required.            |
| `unsupported_operation`   | Missing capability or wrong auth flow; remove unavailable action.        |
| `authentication_required` | Sign in for this operation.                                              |
| `session_expired`         | Source rejected saved/current credentials; show sign-in, not empty list. |
| `challenge_expired`       | Restart sign-in.                                                         |
| `invalid_cursor`          | Restart the relevant list, not another source's list.                    |
| `not_found`               | Unknown track/playlist.                                                  |
| `network`                 | Recoverable upstream connection failure.                                 |
| `rate_limited`            | Retry only after optional positive `retry_after_ms`.                     |
| `busy`                    | Concurrency bound reached; user can retry.                               |
| `cancelled`               | Work cancelled; no success side effects.                                 |
| `source_disabled`         | Host rejected dispatch to disabled source; never sent to plugin.         |
| `source_unavailable`      | Host could not start or lost process; no NetEase fallback.               |
| `internal`                | Sanitized source failure; never expose upstream credential-bearing data. |

Every error includes `code`, plain `message`, and boolean `retryable`.
Optional `retry_after_ms` is meaningful only for retryable errors. `network`,
`rate_limited`, `busy`, and `source_unavailable` can be retryable; other codes
are false. Installation/signature/revocation failures are host workflow states,
not permission for a plugin to activate another version.

## Design review against both sources

| Concern         | NetEase package design                                                   | Executable `fixture` package                                                              |
| --------------- | ------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------- |
| Identity        | `netease`; same existing Streaming Track Identity                        | `fixture`; tracks `1`, `2`, `3`, playlist `42` deliberately collide with NetEase test ids |
| Auth            | Isolated official page, MUSIC_U/__csrf, `user_account` verification      | QR/text prompt, opaque challenge, pending then complete polls; deterministic test account |
| Capabilities    | All seven capabilities, authenticated browse/resolve                     | Playlists, playlist tracks, details and import only; no search, liked tracks or preview   |
| Pagination      | Plugin converts upstream offsets/id batches; can cache bounded snapshots | Uneven 1-then-2 track pages; opaque session-bound single-use cursors, no full id list     |
| Metadata        | Converts upstream titles/artists/privileges and CDN artwork URLs         | Distinct known titles/artists and generated artwork; no NetEase-shaped fields             |
| Media           | `song_url`, full-track checks, China Client Address, no UNM              | Full generated WAV descriptors; track `2` is a trial refusal; no vendor credentials       |
| Expiry/errors   | Upstream auth expiry distinct from refusal/network failure               | Restored expired credential scenario, cancelled/expired challenges, network fault mode    |
| Progress/cancel | Resolution/upstream batch progress before result                         | Delayed resolution with correlated progress and cancellable completion                    |
| Ownership       | Host downloads, previews, imports, stores credentials                    | Same ownership; fixture has no library writer or host imports                             |

Review result: no host method requires NetEase batch sizes, numeric ids, cookie
keys, or authentication UI. The second source can omit preview/search without
inventing no-op actions. This is a contract review, not an executed host proof.
See [fixture usage and real-process smoke](../../../fixtures/streaming-source/README.md).
The approved permanent behavioral seam remains application-facing workflows
with the real host and stdio process. No protocol helper/unit test substitutes
for that acceptance. Those workflows arrive in #554/#555; this slice does not
change the production host/browser or claim installed-platform acceptance.
