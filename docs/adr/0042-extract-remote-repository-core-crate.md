# ADR 0042 — Extract a Tauri-free Remote Repository core crate

Date: 2026-04-18
Status: accepted

## Context

[ADR 0011](./0011-remote-library-mirror-database-first.md) defines the database-first **Remote Repository** model. [ADR 0018](./0018-remote-repository-module-ownership.md) assigns each **Remote Repository** lifecycle concern to one module. [ADR 0040](./0040-rebase-song-scoped-changes-on-publish.md) defines song-scoped rebase rules for **Publish Changes**. However, the implementation lived inside `src-tauri/src/remote/` and depended on application `CommandError` types and desktop shell modules. Spec `#590` requires a portable core that desktop and phone builds share. A phone opens a **Read-Only Local Working Copy** and performs **Refresh Repository**, but never performs **Publish Changes**. Moreover, unit tests in `src-tauri` had to link the full Tauri desktop crate.

## Decision

Turn `src-tauri` into a Cargo workspace and extract `crates/openkara-remote` as a Tauri-free library crate. `openkara-remote` owns the **Remote Repository** manifest format, core error types, storage port, conformance suite, read-only **Refresh Repository** engine, and **Publish Changes** engine. `openkara-remote` uses only platform-neutral dependencies and never depends on `tauri` or `CommandError`. The application crate in `src-tauri` owns Tauri commands, OS keychain integration, OAuth loopback flows, and background workers. The application crate maps `openkara_remote::RemoteError` to `CommandError` only at the IPC boundary.

## Consequences

- `cargo test -p openkara-remote` runs standalone without Tauri, network access, or **Repository Credentials**.
- Desktop and future mobile targets compile the same `openkara-remote` crate for **Refresh Repository** and **Offline Pin** rules.
- `openkara-remote` must not import `tauri`, desktop window state, or IPC error types.
- New **Remote Provider** adapters and protocol rules must pass unit and conformance tests inside `openkara-remote` first.
- [ADR 0018](./0018-remote-repository-module-ownership.md) module ownership continues to apply across the workspace boundary.
