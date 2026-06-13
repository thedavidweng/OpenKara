# Release Modernization Decisions

**Date:** 2026-06-12

This document records release-adjacent decisions that are current project
reality, not active implementation plans.

## macOS Notarization And Signing

The release workflow keeps the current Tauri-managed signing path. If Apple
Developer ID secrets are configured, `tauri-apps/tauri-action` can perform
codesigning and notarization through Apple's current notarytool flow.

No v0.9.0 code change is required beyond keeping release notes honest about
whether a specific artifact was signed and notarized.

## Windows Authenticode

Windows installers are not Authenticode-signed unless the repository is
configured with a code-signing certificate and signing secrets.

Windows signing is deferred until distribution volume justifies the certificate
and key-management cost. Release notes must keep describing unsigned Windows
artifacts as an expected limitation until the workflow actually signs them.

## Tauri Updater

OpenKara currently uses manual update distribution through GitHub Releases and
package channels. The Tauri updater plugin is not wired into this repository.

Adding an updater remains post-v1.0 feature work because it requires an update
endpoint, Ed25519 signing-key management, channel policy, and Settings UI.

## Ubuntu ARM Runner

Linux CI currently targets x86_64 release artifacts. ARM64 Linux builds remain
deferred because they require a native or cross-compilation path, matching ONNX
Runtime artifacts, and separate package output.

## Summary

| Area                 | Decision                        | Release blocker |
| -------------------- | ------------------------------- | --------------- |
| macOS signing        | Keep Tauri-managed flow         | No              |
| Windows Authenticode | Defer until certificate exists  | No              |
| Tauri updater        | Defer to post-v1.0 feature work | No              |
| Ubuntu ARM runner    | Defer ARM64 Linux artifacts     | No              |
