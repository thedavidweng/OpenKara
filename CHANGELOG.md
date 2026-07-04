# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed

- Timed lyrics no longer freeze during transient buffering states or when consecutive lines resolve to the same scroll target. A unified lyrics engine now drives highlight sync, karaoke fill, line springs, and auto-scroll from a single `requestAnimationFrame` loop using `translateY` transforms instead of competing `scrollTop` mutations or per-frame React state updates.

### Removed

- Deprecated `useLyricsSync`, `useLyricsAutoScroll`, and `lyrics-playback-clock` shims after migrating all lyrics runtime behavior into `useLyricsEngine` / `lyrics-engine.ts`.

### 📝 Documentation

- Update CHANGELOG for v0.9.0
