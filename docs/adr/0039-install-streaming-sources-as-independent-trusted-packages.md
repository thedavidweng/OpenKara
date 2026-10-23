# ADR 0039 — Install Streaming Sources as independent trusted packages

Date: 2026-10-20
Status: accepted

## Context

NetEase exists on the unreleased #418 branch. Its bundled Node runtime and API
dependencies couple upstream repairs to application releases. A second source
must use the same browser and shared import path without application changes.
This decision implements the design in [#551](https://github.com/thedavidweng/OpenKara/issues/551).

## Decision

Supersede only ADR 0037's bundled-sidecar distribution and build preparation.
Retain its isolated official-page sign-in, cookie capture, account verification,
and window-close rules. Retain ADR 0035's China Client Address and no-UNM
rules, ADR 0034's shared Streaming Import path, and ADR 0036's built-in YouTube
player. Online Preview keeps the existing playback coordinator and bounded URL
refresh. Its current lyric lookup stays in the host; ADR 0038's statement that
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

The initial unreleased trust roots were rotated before source publication:
current `0374d430d3cf1aa6` and next `71a135edb0fedcf0`. Their raw Ed25519
public keys are part of the host contract. The matching private keys are
dedicated to source releases, not application updates. GitHub Actions stores
them in `openkara-sources`; local recovery copies are in macOS Keychain.
BWS custody must be verified before the first production publication. A
configured Actions secret alone does not establish BWS backup.

Support the current and previous protocol major. An incompatible installed or
pinned package needs an update; keep its credentials and enablement. Verified
revocations override pins. Stop revoked code at the next safe point and retain
the last verified revocations when discovery fails. Other discovery failures do
not remove a working installation. Start on first use, multiplex requests, stop
when idle, and bound crash restart backoff. Installation, enablement, runtime
readiness, and authentication are separate states. Disable retains credentials;
sign-out removes them; uninstall offers their removal and keeps library songs.

Use a project-managed runtime pinned by each signed release, never inherited
PATH or system npm. Runtime delivery depends on the application channel:

| Channel                                   | Runtime delivery                                                                                                                                                |
| ----------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| macOS, Windows, Linux AppImage, Linux deb | Download and verify the pinned runtime from the signed source index.                                                                                            |
| Flatpak                                   | Bundle a fixed pinned Node runtime in the application package. Download only signed, verified JavaScript source packages at runtime, never executable runtimes. |

Both modes check the runtime version, target, and compatibility against the
signed source release. A bundled runtime skips the download, not validation.
A source that requires an unavailable bundled runtime version needs a
compatible source or application update; do not download an executable as a
hidden fallback.

The production macOS downloader does not add `com.apple.quarantine` to its
downloads or extracted executable. Record that behavior and check it in the
runtime regression. Do not simulate a browser download or remove attributes
to make a check pass. Never ask users to bypass platform protection.

Record actual distributed runtime feasibility under
[#553](https://github.com/thedavidweng/OpenKara/issues/553), including Linux
AppImage and deb, distribution, FUSE environment, and runtime digests. Exercise
Intel macOS when a runner is available; otherwise record the gap under
[#557](https://github.com/thedavidweng/OpenKara/issues/557). Flatpak sandbox
acceptance and its pnpm tarball build repair also belong to #557, not #553.

OpenKara has not submitted to Flathub; see the
[packaging boundary](../../packaging/flatpak/README.md). There is no Flathub
review conclusion. Downloadable JavaScript plugins are a first-submission
policy risk, not a condition for closing #553. If the first submission rejects
those downloads, ship Flatpak without Streaming Sources and state that limit
in Settings and release notes. Actual sandbox execution and Flathub policy
acceptance are separate evidence; neither implies the other.

### Channel feasibility evidence

The accepted #553 evidence supports the delivery choices above, not every OS
or security reputation state. Durable issue reports record exact application
and runtime hashes, OS versions, and install/restart parent-process checks:

- [macOS ARM64 and native Intel](https://github.com/thedavidweng/OpenKara/issues/553#issuecomment-6301606565):
  signed, notarized candidates; Gatekeeper assessments enabled; no quarantine
  on the downloaded archive, extracted executable, installed runtime, or
  restarted runtime. The application-facing regression fails if this changes.
- [Windows 11](https://github.com/thedavidweng/OpenKara/issues/553#issuecomment-6300784567):
  actual installed candidate; Defender and real-time protection enabled;
  no exclusions. The tested application candidate is unsigned; this does not
  establish other signing or reputation states.
- [AppImage and deb on Ubuntu 22.04.5](https://github.com/thedavidweng/OpenKara/issues/553#issuecomment-6304283709):
  the AppImage ran from its actual FUSE mount with `libfuse2` 2.9.9-5ubuntu3;
  deb execution matched the supplied package payload. Both downloaded and
  restarted the verified runtime. No runtime-delivery fallback is required.

Flatpak uses the maintainer-selected bundled-runtime policy. Its actual
sandbox evidence and pnpm tarball repair remain #557 acceptance work. There
is no Intel runner gap and no Flathub approval claim.

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
