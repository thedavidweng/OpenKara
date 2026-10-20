# ADR 0038 — Install Streaming Sources as independent trusted packages

Date: 2026-10-20
Status: accepted

## Context

NetEase exists on the unreleased #418 branch. Its bundled Node runtime and API
dependencies couple upstream repairs to application releases. A second source
must use the same browser and shared import path without application changes.
This decision implements the design in [#551](https://github.com/thedavidweng/OpenKara/issues/551).

## Decision

Supersede only ADR 0035's bundled-sidecar distribution and build preparation.
Retain its isolated official-page sign-in, cookie capture, account verification,
and window-close rules. Retain ADR 0033's China Client Address and no-UNM
rules, ADR 0032's shared Streaming Import path, and ADR 0034's built-in YouTube
player. Online Preview keeps the existing playback coordinator and bounded URL
refresh. Its current lyric lookup stays in the host; ADR 0036's statement that
preview has no lyric lookup is not the implemented behavior.

Publish reviewed packages in `openkara-sources`, with independent releases.
The initial host accepts only project-controlled releases. Bind the stable
source id to a verified publisher/package pair, not a self-declared manifest.
NetEase keeps `netease`. Package identity cannot acquire another source's
Streaming Credentials or stored track identities.

Use the [versioned stdio contract](../references/contracts/streaming-source-protocol.md).
A separate managed Node process owns upstream calls and metadata conversion.
It imports no application internals and supplies no executable UI. The host
owns credentials persistence, official-page windows, resources, media downloads,
Online Preview, Streaming Import, and library mutation. Plugins return HTTPS
media descriptions, never arbitrary local files or audio in JSON.

The signed source index binds each immutable package archive to its publisher,
source id, version, size, SHA-256, protocol, host requirement, runtime pin and
per-platform runtime digests. Use a dedicated source-signing key, separate from
the application updater key. Embed current and next public keys for rotation.
Verify the signature and all bytes before execution. Resolve and lock production
dependencies at publication. Do not run npm or lifecycle scripts on clients.
Install runtime and package versions into immutable directories. A successful
compatible handshake is required before staged activation replaces a usable
version. Updates require user action and wait for active work to finish.

Support the current and previous protocol major. An incompatible installed or
pinned package needs an update; keep its credentials and enablement. Verified
revocations override pins. Stop revoked code at the next safe point and retain
the last verified revocations when discovery fails. Other discovery failures do
not remove a working installation. Start on first use, multiplex requests, stop
when idle, and bound crash restart backoff. Installation, enablement, runtime
readiness, and authentication are separate states. Disable retains credentials;
sign-out removes them; uninstall offers their removal and keeps library songs.

Use a project-managed runtime pinned by each signed release, never inherited
PATH or system npm. Platform feasibility is a separate release gate under
[#553](https://github.com/thedavidweng/OpenKara/issues/553). A distributed macOS,
Windows, or Flatpak run is not established by this ADR. If executable downloads
are not viable, prefer a channel-bundled pinned runtime and downloaded JavaScript
packages. If that also fails, explicitly ship that channel without Streaming
Sources. Flathub policy acceptance is separate from sandbox execution. Never ask
users to bypass platform protection.

Keep application code AGPL-3.0-only. MIT is the intended license for plugin-owned
code and protocol development materials only where sufficient rights exist.
The [provenance audit](../references/streaming-source-provenance.md) records the
actual history and outstanding rights evidence. Until that evidence exists,
this contract and executable fixture remain application AGPL-3.0-only materials.
Retain historical Apache grants and third-party licenses and notices. A CLA
file alone does not license earlier contributions. Separate processes, narrow
stdio, independent artifacts, and no internal imports are engineering and
distribution boundaries, not a conclusion about combined or derived works.

## Consequences

- The deterministic second-source package exercises fewer capabilities,
  cursor pages, plugin-driven sign-in, and ids that collide with NetEase.
  It is test infrastructure, not a production music service.
- Authorized plugins are trusted code. They can disclose credentials supplied
  to them. Processes, signatures, and credential namespaces are not a sandbox.
- The first released NetEase implementation is the plugin. There is no
  built-in-to-plugin migration. Developer branch data is cleaned manually.
  Revisit this decision if #418 ships before the cutover.
- Remove the old worker, bundled runtime, and build paths only after the host
  and published NetEase package work. No parallel compatibility path remains.
- YouTube script updates have separate acceptance and do not block this work.
