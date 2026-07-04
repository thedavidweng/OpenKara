# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed

- Style Windows and Linux scrollbars to match the dark desktop shell instead of showing light WebView2/WebKitGTK tracks ([#51](https://github.com/thedavidweng/OpenKara/issues/51)).

### Security

- Bump direct `quick-xml` to 0.41 and document scoped `cargo-deny` ignores for the residual Tauri/plist 0.39.x chain (RUSTSEC-2026-0194/0195).
- Bump `anyhow` to 1.0.103 (RUSTSEC-2026-0190).

## [0.9.0] - 2026-06-14

### 📝 Documentation

- Update CHANGELOG for v0.9.0
