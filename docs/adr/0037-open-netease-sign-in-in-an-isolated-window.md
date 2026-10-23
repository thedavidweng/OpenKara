# ADR 0037 — NetEase web sign-in and bundled API sidecar

Date: 2026-09-28
Status: accepted for controlled official-page authentication; the bundled-sidecar
decision is superseded by [ADR 0039](0039-install-streaming-sources-as-independent-trusted-packages.md)

## Context

NetEase does not offer the desktop sign-in and song import flow through its
developer platform. OpenKara's own password, SMS, QR, request encryption, and
endpoint code duplicated a changing provider protocol. Users need a way to
sign in without entering verification material into OpenKara and to keep an
accepted full song as a local library file.

## Decision

OpenKara opens the official NetEase page in a dedicated incognito Tauri window
with no application capability. Rust watches only `MUSIC_U` and `__csrf` while
the window is open. It verifies the account before saving those cookies and the
display name in the existing system credential store. The window closes after
verification succeeds or fails, and sign-out closes any open login window.

NetEase account, playlist, search, detail, and URL operations use a pinned
release of `@neteasecloudmusicapienhanced/api` in one app-owned Node process.
The process exchanges one JSON request and response per line on standard
input/output. It has no local HTTP listener. The sidecar owns the upstream
protocol and uses its China Client Address option, preserving ADR 0035.
OpenKara downloads only a full permitted URL and passes the temporary file to
the existing Streaming Import path from ADR 0034. It never tries an alternate
music source after refusal.

## Consequences

- macOS, Windows, and Linux packages include the pinned Node runtime and npm
  dependency tree. Build preparation verifies the runtime SHA-256 and uses the
  lockfile integrity records for dependencies.
- Upstream breakage and an unavailable sidecar are visible as errors. A
  rejected or expired account is not silently replaced with another identity.
- The official page and platform webview can change. Sign-in and installed
  sidecar behavior need manual acceptance on each desktop platform before
  release.
- The only persisted NetEase data is the session cookies and display name;
  sign-out deletes them. Audio files enter the library only after the shared
  import path accepts them.
