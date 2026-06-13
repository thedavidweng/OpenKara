---
name: ci-notes
description: CI environment constraints, CodeQL patterns, and Cursor Cloud notes. Use when editing CI workflows, debugging CodeQL alerts, or working in Cursor Cloud.
---

# CI Notes

## GitHub Actions constraints

- Every workflow file must have an explicit top-level `permissions:` block (typically `contents: read`).
- Keep `actions/checkout` and `actions/setup-node` on supported major versions.
- Preserve Linux native packages required by Tauri and audio builds when editing CI.
- If all Verify jobs fail quickly on every OS, check whether they all failed at the same step before debugging platform-specific causes. If the shared failure step is formatting, assume a repo-wide formatting issue first.

## Windows CI: ONNX Runtime

- Windows job MUST include ONNX Runtime preparation steps even though only `cargo test --no-run` runs.
- Reason: Tauri's build script validates `bundle.resources` paths at **compile time**, not runtime. Without ONNX Runtime, compilation fails with `resource path generated\onnxruntime doesn't exist`.
- Windows Rust integration tests cannot run on GitHub Actions (headless Server 2025 lacks desktop DLL APIs). This is a test-environment limitation, not a code bug.

## CodeQL patterns

- **Cleartext logging (Rust):** Use `error.without_url()` instead of `{error}` when formatting `reqwest::Error`. CodeQL traces OAuth tokens through `authorized_request` helpers.
- **Unsafe pointer access (Rust):** Prefer `ptr::NonNull::new(p)` + `.as_ref()` over raw `&*p`.
- **Tauri `generate_handler!` false positives:** CodeQL cannot model Tauri's macro-generated IPC dispatch. Dismiss "hard-coded cryptographic value" alerts inside `generate_handler!` as false positives.

## Cursor Cloud

- Node.js **24**（与 GitHub Actions `node-version: 24` 一致；仓库根目录 `.nvmrc` / `.node-version`）
- System packages (libwebkit2gtk, libasound2-dev, etc.) and Rust stable are pre-installed in the VM snapshot.
- `pnpm tauri dev` needs a display (X11/Wayland) to open the WebView window.
- `./scripts/setup.sh` downloads model + ONNX Runtime (~80 MB). Idempotent.
- Rust 1.85+ required (edition2024 support for `time` crate).
