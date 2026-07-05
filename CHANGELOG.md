# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- Tooltip hover UX: 600ms first-show delay, instant switching between adjacent triggers, and extended hit areas for gap bridging. Play/pause transport controls no longer show redundant tooltips.

### Fixed

- Extract `[offset:]` metadata from LRC content in all lyrics caching paths ([#60](https://github.com/thedavidweng/OpenKara/issues/60)): `save_manual_lyrics`, `extract_embedded_lyrics`, `fetch_lyrics`, and `fetch_lyrics_online` were storing `offset_ms: 0` ignoring any `[offset:...]` tag in the LRC content. Now all four paths extract the offset via `parse_lrc_metadata`, matching the existing `import_lyrics_files` behavior.
- Style Windows and Linux scrollbars to match the dark desktop shell instead of showing light WebView2/WebKitGTK tracks ([#51](https://github.com/thedavidweng/OpenKara/issues/51)): Windows uses Tauri `scrollBarStyle: fluentOverlay` (WebView2 overlay scrollbars) plus scoped dark `scrollbar-*` / `::-webkit-scrollbar` CSS; Linux keeps CSS-only styling.
- Stop lyrics IPC commands from freezing the UI main thread. `fetch_lyrics`, `fetch_lyrics_online`, `set_lyrics_offset`, `save_manual_lyrics`, `import_lyrics_files`, and `extract_embedded_lyrics` were synchronous Tauri commands, which Tauri 2 runs on the webview main thread. They performed blocking file I/O (lofty embedded-lyrics reads) and `reqwest::blocking` network calls (LRCLIB/LrcApi fetches plus remote-library revision checks, database sync, and song publish), causing a ~1s rainbow-cursor freeze when web-lyrics lookup failed and fell back to embedded lyrics. All six commands are now `async`. `fetch_lyrics` and `fetch_lyrics_online` use async `reqwest` for network I/O (no `spawn_blocking` thread occupied during HTTP requests) and `spawn_blocking` only for short DB/file operations. The other four commands use `spawn_blocking` for their blocking work.

### Security

- Bump direct `quick-xml` to 0.41 and document scoped `cargo-deny` ignores for the residual Tauri/plist 0.39.x chain (RUSTSEC-2026-0194/0195).
- Bump `anyhow` to 1.0.103 (RUSTSEC-2026-0190).

## [0.9.0] - 2026-06-14

### 📝 Documentation

- Update CHANGELOG for v0.9.0
