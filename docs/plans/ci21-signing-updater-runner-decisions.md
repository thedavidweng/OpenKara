# CI21: Signing / Updater / Runner Modernization Decisions

**Date:** 2026-06-12
**Status:** Decision document (evaluation only)

## macOS Notarization & Signing

**Current state:** Tauri v2 action handles codesigning and notarization via
Apple Developer ID certificate passed through `APPLE_CERTIFICATE` /
`APPLE_CERTIFICATE_PASSWORD` / `APPLE_ID` / `APPLE_PASSWORD` secrets.

**Decision:** Keep the current Tauri-managed signing flow. No changes needed
for v0.9.0. The `tauri-apps/tauri-action@action-v0.6.2` already supports
notarytool (replaced altool in Xcode 14+). No action required.

**Follow-up:** None for this release.

## Windows Authenticode

**Current state:** Windows builds produce unsigned installers. Authenticode
signing requires an EV code signing certificate and a hardware token or
cloud HSM service (e.g., SSL.com eSigner, DigiCert KeyLocker).

**Decision:** Defer to post-v1.0. Acquiring an EV certificate is a business
decision that depends on distribution channel requirements (SmartScreen
reputation builds over time with enough unsigned downloads; WinGet does not
require signing). Document this as a known gap in release notes.

**Follow-up:** Open a tracking issue for Windows code signing when
distribution volume justifies the cost.

## Tauri Updater

**Current state:** The app uses `tauri-plugin-dialog` for manual update
checks. The Tauri updater plugin (`tauri-plugin-updater`) provides
background update checking and differential updates via a signed update
manifest.

**Decision:** Evaluate `tauri-plugin-updater` for post-v1.0. The current
manual-download-per-release workflow via GitHub Releases is acceptable for
the current release cadence. Adding the updater plugin requires:

1. An update endpoint (GitHub Releases manifest or custom server).
2. Ed25519 signing key management.
3. Update channel configuration (stable/beta).
4. A Settings UI for auto-update preferences.

This is feature work, not a v0.9.0 audit item. Close CI21 with this
document; open a feature issue if the team wants automatic updates.

## Ubuntu ARM Runner

**Current state:** Linux CI builds run on `ubuntu-22.04` (x86_64 only).
There is no ARM64 Linux build in CI.

**Decision:** Defer ARM64 Linux builds. GitHub-hosted ARM runners
(`ubuntu-24.04-arm`) are available but still in preview for some orgs.
The Flatpak and deb packages target x86_64 only. ARM64 Linux support
requires:

1. A cross-compilation or native ARM64 build step.
2. ARM64 ONNX Runtime binaries (already available upstream).
3. Flatpak ARM64 variant or separate ARM deb.

This is a post-v1.0 item. Close CI21 with this document; track ARM64
Linux as a separate feature request.

## Summary

| Area                 | Decision                        | Blocker? | Follow-up                                 |
| -------------------- | ------------------------------- | -------- | ----------------------------------------- |
| macOS signing        | Keep current Tauri-managed flow | No       | None                                      |
| Windows Authenticode | Defer to post-v1.0              | No       | Open tracking issue when volume justifies |
| Tauri updater        | Evaluate post-v1.0              | No       | Open feature issue if desired             |
| Ubuntu ARM runner    | Defer                           | No       | Track as feature request                  |
