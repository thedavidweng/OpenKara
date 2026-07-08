# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- Deepen frontend Playback Session architecture (maintainer-visible): extract `src/playback/` session module (play/skip/onEnded/clock) with thin player-store adapter; pure audience projector for AirPlay assembly. No user-visible playback behavior change intended.
- Deepen the Rust `library` module write path: import, delete, song metadata/flags, and playlist/rotation logic now live under `src-tauri/src/library/` (`import/`, `delete`, `songs`, `playlist`). `commands/import` and `commands/playlist` are thin IPC adapters that open the DB and wrap remote `run_*_mutation` hooks. Remote mirror sync calls `library::delete_*` instead of `commands::import::delete`.
- Deepen Remote Repository domain architecture (maintainer-visible): lift domain body from `commands/remote_library` into crate-root `remote/` with thin IPC adapters; close the auth/registry Remote Provider seam via `ProviderSessionData` + shared credential binding for Register/Reauthorize; collapse triplicated initialize/refresh bootstrap into one `bootstrap_remote_library` protocol (`CreateOrOpen` | `RequireExisting`) with provider HTTP/path adapters. Public Tauri command names and IPC contracts are unchanged.
- Move stem-separation lifecycle orchestration out of Tauri command handlers into `services::separation` (status DTOs, bootstrap prerequisites, single/batch job launching, terminal publish). Command modules are thin IPC adapters; IPC names and event payloads are unchanged.
- Tooltip hover UX: 600ms first-show delay, instant switching between adjacent triggers, and extended hit areas for gap bridging. Play/pause transport controls no longer show redundant tooltips.

### Fixed

- Wrap mirror sync DB deletes in a SQLite transaction ([#65](https://github.com/thedavidweng/OpenKara/issues/65)): `sync_bound_remote` deleted songs from the remote database one at a time without a transaction. If a delete failed mid-loop, the remote DB was left in a partial state (some songs deleted, others not). Now all DB deletes happen in a single transaction — if any fails, all deletes roll back. Cloud file deletes happen after the transaction commits (best-effort), so a failure there leaves orphaned files (wasted storage) rather than DB entries pointing at missing files.
- Style Windows and Linux scrollbars to match the dark desktop shell instead of showing light WebView2/WebKitGTK tracks ([#51](https://github.com/thedavidweng/OpenKara/issues/51)): Windows uses Tauri `scrollBarStyle: fluentOverlay` (WebView2 overlay scrollbars) plus scoped dark `scrollbar-*` / `::-webkit-scrollbar` CSS; Linux keeps CSS-only styling.
- Stop lyrics IPC commands from freezing the UI main thread. `fetch_lyrics`, `fetch_lyrics_online`, `set_lyrics_offset`, `save_manual_lyrics`, `import_lyrics_files`, and `extract_embedded_lyrics` were synchronous Tauri commands, which Tauri 2 runs on the webview main thread. They performed blocking file I/O (lofty embedded-lyrics reads) and `reqwest::blocking` network calls (LRCLIB/LrcApi fetches plus remote-library revision checks, database sync, and song publish), causing a ~1s rainbow-cursor freeze when web-lyrics lookup failed and fell back to embedded lyrics. All six commands are now `async`. `fetch_lyrics` and `fetch_lyrics_online` use async `reqwest` for network I/O (no `spawn_blocking` thread occupied during HTTP requests) and `spawn_blocking` only for short DB/file operations. The other four commands use `spawn_blocking` for their blocking work.

### Security

- Bump direct `quick-xml` to 0.41 and document scoped `cargo-deny` ignores for the residual Tauri/plist 0.39.x chain (RUSTSEC-2026-0194/0195).
- Bump `anyhow` to 1.0.103 (RUSTSEC-2026-0190).
- Bump transitive `crossbeam-epoch` to 0.9.20 (RUSTSEC-2026-0204): the `fmt::Pointer`/`fmt::Display` impls on `Atomic`/`Shared` dereferenced the underlying pointer, causing a null-pointer dereference for `Atomic::null`/`Shared::null`. Pulled in via `rayon` → `rayon-core` → `crossbeam-deque`; updated with `cargo update -p crossbeam-epoch` (semver-compatible patch, no `Cargo.toml` change).
- Resolve all 72 open `zizmor` code-scanning alerts on `main`: pin SHA-pinned actions to tagged versions (`codecov/codecov-action` v5.5.5, `dtolnay/rust-toolchain` v1, `taiki-e/install-action` v2.73.0), add version comments to all SHA pins, disable cache saves in the release workflow (`lookup-only: true`, `package-manager-cache: false`) to prevent cache poisoning, move `contents: write` to job-level in `dependabot-sync.yml`, use `github.event.pull_request.user.login` instead of spoofable `github.actor` for the Dependabot bot check, add `concurrency` blocks to `mirror.yml` and `dependabot-sync.yml`, document all `contents: write` permissions, and add a 7-day Dependabot cooldown.

## [0.9.0] - 2026-06-14

### 📝 Documentation

- Update CHANGELOG for v0.9.0
