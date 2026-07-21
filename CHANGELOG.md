# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changelog

- Record romanize fullscreen sync (#142)

### ♻️ Refactoring

- Introduce PlaybackCoordinator as independent control thread (#100)
- Architecture deepening across 7 module-depth candidates (#129)
- Consolidate duplicate utilities and inline single-caller hooks (#152)

### ⚡ Performance

- **lyrics**: Reuse HTTP clients across fetches for connection pool sharing (#74)

### ✨ New Features

- **windows**: Enable fluentOverlay scrollbars per Tauri guidance
- **ui**: Delayed tooltip UX with instant adjacent switching (#54)
- **audio**: Optimize decode, output, streaming, and cache pipeline (#43)
- **lyrics**: Show matched song name in toast on .lrc import (#95)
- Add NextTrack pipeline with gapless playback (#103)
- **library**: Add persistent song list sort modes (#93)
- **library**: Add alphabet rail for fast library navigation (#94)
- **ui**: Light mode, playback chrome, and volume sliders
- Generate cover art thumbnail and preview derivatives in library root (#91) (#107)
- **cdg**: Overhaul CD+G playback for correctness and robustness (#113)
- Add realtime peak envelope visualizer (#87) (#102)
- Add managed-library integrity audit and transactional cleanup (#128)
- Add waveform seekbar with composite PK cache and singleflight (#90) (#106)
- **website**: Embed a live OpenKara app preview (#123)
- **website**: Add Codex to Built With logo wall
- **website**: Remove preview details section from landing page
- Unify mock song data and IPC mechanism between website preview and E2E tests (#132)
- Add equal-power crossfade between consecutive tracks (#131)
- **romanize**: Sync romanized lyrics to fullscreen audience window (#142)
- **remote**: Durable remote-state.db control plane with operation state machine and startup recovery (#151)
- **remote**: Versioned manifest, transactional publish, CAS conflict handling
- **remote**: Resumable provider transfers and shared network retry policy (#151)
- **remote**: Persistent verified cache catalog with bounded eviction (#151)
- **remote**: Playback reconnect and source replacement with timeline preservation (#151)
- **remote**: Frontend states, conflict actions, diagnostics, and fault-injection suite (#151)
- **remote**: Verify referenced assets before manifest CAS (#151)
- **remote**: Manifest-based pull, GC executor, resumable uploads, range downloads (#151)

### 🐛 Bug Fixes

- Use gh release download for SHA256SUMS and ignore git-cliff in knip
- Format CHANGELOG with oxfmt after git-cliff generation
- Use ignoreBinaries for git-cliff in knip config
- Eliminate script injection risk in release workflow and update tests
- Clamp tooltip left position when wider than viewport
- Resolve CI failures and review feedback
- Resolve TypeScript errors in test files
- Pin action SHA, remove test.txt, add permissions
- Correct mirror action SHA
- Add force_push and fetch-depth: 0
- Override undici to >=7.28.0 to resolve high-severity vulnerabilities
- Constrain undici to 7.x, fix workflow security, update flatpak sources
- Revert knip/tsgo/vitest to pnpm (no matching package.json scripts)
- **ui**: Style desktop scrollbars for dark shell on Windows/Linux
- **ci**: Bump quick-xml and sync Flatpak cargo vendor for deny
- **ci**: Patch anyhow and ignore residual quick-xml advisories
- **ui**: Lighten scrollbar thumb hover on desktop
- **ui**: Apply desktop scrollbar props to scroll descendants
- **lyrics**: Unify timed lyrics engine and fix freeze stalls (#56)
- **ci**: Sync knip ignoreBinaries and regenerate Flatpak cargo sources
- **lyrics**: Eliminate UI main-thread freeze on lyrics fallback (#67)
- **lyrics**: Omit duration=0 from LRCLIB lookup for unknown duration (#68)
- **lyrics**: Propagate cache write errors in import_lyrics_files (#69)
- **lyrics**: Case-insensitive sidecar extension matching (#71)
- **audio**: Use actual consumed frames in multi-stem streaming render (#73)
- **lyrics**: Add 7-day TTL to negative cache for online re-lookup (#75)
- **remote**: Recover active_library_id after crash during mirror sync (#76)
- **lyrics**: Extract [offset:] metadata in all lyrics caching paths (#70)
- **lyrics**: Optimistic offset update to prevent race on rapid clicks (#72)
- **remote**: Wrap mirror sync DB deletes in a SQLite transaction (#77)
- **deps**: Bump crossbeam-epoch to 0.9.20 (RUSTSEC-2026-0204)
- **flatpak**: Sync cargo-sources.json with crossbeam-epoch 0.9.20
- **ci**: Resolve all 72 zizmor code-scanning alerts
- **ci**: Add toolchain: stable to dtolnay/rust-toolchain v1
- **ci**: Add tool: nextest to taiki-e/install-action v2.73.0
- **ci**: Bump zizmor-action to v0.5.7 (zizmor v1.26.1)
- **lyrics**: Restore auto-scroll after seek and mid-song drift (#84)
- **ci**: Authenticate WinGet manifest render API calls
- **macos**: Detach leftover DMG mounts before cleaning bundle dir
- **settings**: Enforce platform-scoped execution-provider choices (#115) (#117)
- **ci**: Upgrade pnpm 10.33.2 → 11.13.0 for retired npm audit endpoint (#120)
- **flatpak**: Bump pnpm tarball to 11.13.0 in manifest template
- **ui**: Unify native-style scrollbar visibility across platforms (#97) (#105)
- **audio**: Restore mainline EQ and gapless safety (#124)
- **ui**: Use accent blue for active Karaoke Library selection (#127)
- **website**: Serve from github.io, redirect xyz, restore favicons, allow lyrics interaction in preview (#130)
- **lyrics**: Blur Follow button after click to release pinned controls
- **website**: Replace em dashes in copy with pipes and rephrasing
- **website**: Tighten spacing between hero actions and app preview
- **website**: Contain preview halo below mock midpoint for smooth fade
- **website**: Enlarge hero action pills to match hero text scale
- **website**: Tighten preview halo and keep Built-with logos on one row (#134)
- **ci**: Cancel search debounce on clear and cap Linux link jobs (#135)
- **playback**: Suppress transport during loading window (#133)
- **website**: Collapse gap between consecutive feature sections
- **website**: Also collapse gap after library feature cards
- **website**: Enlarge closing Download and View source pills
- **website**: Optically center pill CTA labels
- **website**: Center CTA labels with translateY nudge
- **website**: Polish mock preview freeze, counts, sidebar, Separate size
- **website**: Theme/lang auto-detect, mock interactions, natural Chinese copy (#139)
- **audio**: Replace per-stem rendering with source-domain mix bus (#144)
- **romanize**: Invalidate stale romanizedLines cache when source lyrics change
- **website**: Improve mobile landing layout and preview
- **website**: Restore interactive mobile mock with left-half scale
- **website**: Bleed mobile mock past the right viewport edge
- **rotation**: Shuffle within equal-size singer tiers so repeated Shuffle presses vary (#147)
- **website**: Restore desktop preview fill while keeping mobile bleed (#148)
- **cover-art**: Resolve ambience backdrop revocation race on song change (#149)
- **romanize**: Scale pronunciation and bg_words lines with lyricsFontStep (#153)
- **remote**: Route stems_remote around single-file streaming and verify complete stem sets (#151)
- **remote**: Download remote stems in fallback path and clean temps on failure
- **remote**: Unique batch operation ids, filter phantom uploads, migrate fallback DB
- **remote**: Atomic verified downloads and dirty working-copy protection (#151)
- **remote**: Use size-only cache fast-path and sanitize revision in operation id
- **remote**: Reset terminal ops on re-publish, reject missing CAS token, cancel stale batch rows
- **clippy**: Use is_some_and instead of map_or(false, ...)
- **remote**: Revert unimplemented capability flags, reset transfer progress on failure
- **remote**: Skip eviction on verified hits, restore fast-path skip, wire persist_ranges
- **remote**: Preserve reconnect cache pin guard and fetch event listener
- **remote**: Use as_str for diagnostics, regenerate changelog
- **remote**: Apply migrations on fallback in-memory control DB, fix test TS type
- **remote**: Replace park-thread pin leak with RemoteStreamingRuntime
- **remote**: Use UUID operation IDs, stop reusing terminal rows
- **remote**: Start durable operation executor on app startup
- **clippy**: Use sort_by_key with Reverse in control_db
- **remote**: Don't hold control DB Mutex across network I/O
- **remote**: Dispatch Gc operations in startup executor and reload credentials in single-flight waiters
- **remote**: Block on condvar for refresh waiters, guard duplicate model downloads, align contract docs
- **remote**: Enforce one publication protocol and recovery invariants (#151)
- **remote**: Recover accepted CAS after crash and harden remaining reliability gaps (#151)
- **remote**: Close P0 publication recovery, lock scope, and transfer identity gaps (#151)

### 📝 Documentation

- Update CHANGELOG for v0.9.0
- Update CHANGELOG for v0.9.0
- Delete stale docs, keep only contracts and product specs
- **changelog**: Record crossfade regression test backport (#137)

### 📦 Dependencies

- Add Codecov coverage upload
- Use auto-detection for Codecov coverage files
- Add Codecov config and upload coverage
- Add Test Analytics — JUnit upload to Codecov
- Fix codecov/test-results-action version and add coverage exclusions
- Pin codecov-action to commit hash
- Add dependabot config with auto-sync workflow
- Add Codeberg mirror workflow
- Use node --run instead of pnpm for package scripts
- **flatpak**: Reclaim host disk and disable builder cache (#121)
- Triage PR by changed paths and skip irrelevant jobs (#150)
- **release**: Append installation section to GitHub Release Notes (#154)
- Replace paths-filter with checked-in classifier and harden CI Gate (#156)

### 🔧 Chores

- **deps**: Bump @typescript/native-preview
- Replace .nvmrc/.node-version with mise.toml (#38)
- **deps**: Bump all dependencies (supersedes dependabot PRs #33-#36)
- **deps**: Bump @typescript/native-preview from 7.0.0-dev.20260616.1 to 7.0.0-dev.20260622.1 (#46)
- **deps**: Bump the dev-dependencies group across 1 directory with 8 updates (#50)
- **deps**: Bump the production-dependencies group across 1 directory with 2 updates (#49)
- **deps**: Bump i18next from 26.3.3 to 26.3.4 in the production-dependencies group (#79)
- **deps**: Bump the dev-dependencies group with 8 updates (#78)
- **deps**: Bump lucide-react from 1.21.0 to 1.23.0 (#80)
- **deps**: Bump @typescript/native-preview from 7.0.0-dev.20260622.1 to 7.0.0-dev.20260706.1 (#81)
- Bump knip to 6.26 and update ignoreBinaries (#119)
- **deps**: Bump serde_with from 3.18.0 to 3.21.0 in /src-tauri in the cargo group across 1 directory (#122)
- Upgrade npm and Rust dependencies (#140)
- Regenerate changelog
- Regenerate changelog
- Regenerate changelog
- Regenerate changelog
- Regenerate changelog

### 🧪 Tests

- Add 13 new test files for untested modules
- **audio**: Backport crossfade regression tests from original #89 branch (#137)
- **audio**: Assert incoming resampler cache cleared after crossfade abort

## [0.9.0] - 2026-06-14

### Fix

- Add mirror-screenshots-url to flatpak packaging

### ♻️ Refactoring

- Clippy fixes, dependency cleanup, and code modernization
- Playlist/rotation feature, docs restructure, cleanup plan docs
- Decompose AppState into domain modules (PR 1)
- Add RemoteProvider trait seam for cloud storage dispatch
- Replace error string-matching with typed error enums (PR 2)
- Extract playback workflow with dependency injection
- Extract useEventSubscriptions factory hook
- Slim AGENTS.md with progressive disclosure (hooks + skills)
- Move project skills to .agents/skills/, slim AGENTS.md
- Relicense to Apache 2.0 and deepen remote_library architecture

### ✨ New Features

- Complete H1-H8 hardening and F1 playlists/singer-rotation for v0.9
- Redesign singer rotation controls and queue filtering
- Upgrade ONNX Runtime to v1.26.0 with x86_64 macOS fallback
- Extend LyricLine with bg_words and section fields
- Extend TypeScript LyricLine and LyricsSource types
- Add format auto-detection, LrcAPI TTML, sidecar .ttml/.lys
- Add TTML/LYS format detection in lyrics edit dialog
- Add lyrics visual improvements (typography, transitions, glow, karaoke fill, bg vocals)
- Add end_ms to WordToken for precise word duration
- Spring physics for line transitions replacing CSS transitions
- Per-character glow, bg slide-in, last word emphasis
- Migrate website from Jekyll to VitePress with i18n
- Automate release with tag-triggered workflow and git-cliff notes

### 🐛 Bug Fixes

- **flatpak**: Add missing license and high-resolution icon
- **ci**: Add ONNX Runtime prep to Windows compile test job
- Replace safe_request_error with RequestSendExt trait using tracing::trace! for debug logging
- **ci**: Detect and repair broken macOS Rust toolchain
- **ci**: Disable rust-cache cache-bin to prevent toolchain overwrite
- Address 5 PR review issues
- Format docs/generated/db-schema.md for Prettier compliance
- Address greptile sort_order collision and set_queue_entry_singer review
- Wrap advance_rotation read-modify-write in BEGIN IMMEDIATE transaction
- Replace window.prompt() with React InputDialog for Tauri WKWebView compat
- Make Rust cache restores resilient to transient runner failures
- Address Greptile review comments (dead code + FK enforcement)
- Handle missing file in WebDAV download_file
- Match exact review suggestion for download_file
- Ensure database file exists after upload
- Make post-upload missing-file check provider-specific
- Pass app_data_dir to WebDAVProvider, provider-specific error messages
- Handle empty path in GoogleDrive delete_path to delete root folder
- Address PR #21 review comments
- Address PR #23 review comments
- Address PR #24 review comments
- Drain old scheduler before replacement in recreation effects
- Downgrade ONNX Runtime to v1.23.0 for x86_64 macOS support
- Native-feel polish — native menus, system scrollbars, focus rings, audio thread safety
- Pin Windows ONNX Runtime DirectML to 1.24.4
- Guard closest() call in keyboard shortcut dialog check
- Restore --deny-warnings strictness and react/only-export-components rule
- Correct scoped npm tarball URLs in Flatpak node-sources
- Strip pnpm lockfile quotes and regenerate Flatpak node-sources
- Place Flatpak pnpm manifest in node source dir
- Pin @vitest/coverage-v8 to match vitest version and sync Flatpak sources
- Address review comments on test suite
- Add json coverage reporter and restore esbuild Flatpak setup
- Add E2E step to CI and use beforeEach for test state reset
- Adjust coverage thresholds to realistic levels for UI-heavy project
- Resolve CI lint, build, and E2E failures
- Restore Flatpak esbuild cache and address Greptile review
- Revert @vitest/coverage-v8 to exact version to match lockfile
- Restore populate_pnpm_store.py and setup_sdk_node_headers.sh in Flatpak sources
- Address Copilot review comments
- Resolve CI failures from previous commit
- Revert deny.toml values and fix clippy useless_conversion
- Update deny.toml for cargo-deny v2 schema
- Migrate deny.toml to cargo-deny v2 format
- Proper cargo-deny v2 config
- Resolve clippy warnings in new lyrics code
- Upgrade quick-xml to 0.39, remove stale 0.37 from lockfile
- Clear stale spring entries on song change to prevent unbounded memory growth
- Tighten LYS detection to require closing bracket and validate sidecar content
- Remove blur on non-active lyric lines for karaoke readability
- Resolve lyrics review blockers
- Resolve lyrics review blockers
- Preserve ttml word spacing
- Keep karaoke fill and bg vocals readable
- Stabilize lyric animation timing
- Preserve karaoke fill state transitions
- Resolve latest lyrics review gaps
- Audit v0.9.0 follow-up — playback, lyrics, icon, and CI
- Regenerate lockfile and exclude it from oxfmt
- Add comments to clarify magic numbers in test
- Add security-events permission for zizmor SARIF upload
- Prepare v0.9.0 release — merge changelog, update stale references
- Resolve clippy dead-code errors from remote_library refactor
- Update stale action SHAs and fix markdown formatting
- Revert dtolnay/rust-toolchain to compatible version and regenerate flatpak sources
- Regenerate flatpak node-sources from main lockfile
- Address PR review — replace YouTubeLink, add Legal nav, update changelog
- Enable local search provider in VitePress themeConfig
- Align pages.yml action SHAs with main
- Revert taiki-e/install-action to compatible version
- Regenerate flatpak node-sources for PR lockfile (with vitepress)
- Group step summary redirects to satisfy shellcheck SC2129
- Point download links to latest release and add winget install option
- Handle missing liquid-glass assets and fix ORT target env var

### 📝 Documentation

- **AGENTS.md**: Record Windows CI ONNX Runtime requirement to prevent recurrence
- Document cargo fmt requirement for future agents
- Sync v0.8.x status, roadmap links, and future-work backlog
- Lock agreed hardening and v0.9+ feature priorities
- Single active plan under docs/plan; archive priority snapshot
- Merge plan and exec-plans into docs/planning
- Rename planning/ to plan/ and trim hub duplication
- Add architecture deepening design spec
- Add Phase 6 playback-startup latency + beachball root-cause analysis
- Finalize 0.9.0 changelog for release
- Finalize 0.9.0 changelog for release
- Add lyrics enhancement design spec
- Add lyrics enhancement implementation plan
- Add amll-ttml-db and AMLL to acknowledgments
- Add AMLL-inspired visual refinement implementation plan
- Record dependabot esbuild fix
- Add winget install option to README and README_CN

### 📦 Dependencies

- Upgrade runners to ubuntu-24.04 and add quality gates
- Fix workflow lint action
- Align workflows with security policy
- Avoid persisting checkout credentials
- Harden workflow security findings
- Fix winget validation shell env
- Fix windows onnx runtime env expansion

### 🔧 Chores

- Add npm audit auto-block to CI, regenerate flatpak cargo sources
- Improve error conversion section comment
- Add lefthook pre-commit hook for prettier + cargo fmt
- Update GitHub Actions to eliminate Node.js 20 deprecation
- Replace ESLint + Prettier with Oxlint + Oxfmt
- Reorganize docs from superpowers to plans
- Prune stale roadmap archive docs

### 🧪 Tests

- **playback**: Ignore flaky airplay pause/resume test on Linux
- Add 55 meaningful tests for cache layer, lyrics parser, and frontend logic
- Add test pyramid layers with contract, component, E2E tests and coverage reporting
- Expand coverage for context menu builder, errors, stores, and Tauri wrappers
- Expand lyrics-store, settings-store, notification-store, and CDG hook tests
- Expand keyboard shortcuts, settings overlay, queue-store, and window chrome tests
- Expand airplay-runtime and cdg-sync-channel tests
- Add settings-overlay library actions tests
- Expand cover-art, song-media, and cdg-sync tests
- Add LyricLine coverage for bg_words, emphasis glow, and state branches
- Harden release readiness checks

## [0.8.0] - 2026-05-07

### ✨ New Features

- **remote**: Unify remote repository reauthorization

### 🔧 Chores

- Checkpoint existing project changes
- Update dependencies to latest
- Refresh tauri dependencies
- **deps**: Bump tauri

### 🧪 Tests

- Remove unused support target

## [0.7.0] - 2026-04-30

### ♻️ Refactoring

- Separate library management actions from switching in SettingsLibrarySection and remove unused local library binding logic

### ✨ New Features

- Add romanized lyrics display alongside original text

### 🐛 Bug Fixes

- Format docs markdown to satisfy CI prettier checks
- Address 10 bugs and performance issues from code review
- Prevent self-destructive copy when publishing to active remote library
- Apply prettier formatting to tauri.conf.json

### 📝 Documentation

- Migrate roadmap to dedicated status file and add OpenMusic series branding to README
- Keep OpenKara name non-clickable in OpenMusic table

## [0.6.0] - 2026-04-23

### ✨ New Features

- Add remote library registry groundwork
- Implement remote library registration flow with support for WebDAV, Google Drive, and Dropbox providers

### 🐛 Bug Fixes

- Apply Prettier formatting to pnpm-lock.yaml to fix CI
- Make ConfirmationDialog SSR-compatible to fix dialog host test

### 🔧 Chores

- **deps**: Bump the npm_and_yarn group across 1 directory with 3 updates
- **deps**: Bump the cargo group across 1 directory with 2 updates

## [0.5.1] - 2026-04-13

### ✨ New Features

- Tighten provider selection and macos runtime setup
- **macos**: CoreML provider defaults; pin openkara-models v2.0.1
- **separator**: Migrate runtime acceleration to xnnpack

### 🐛 Bug Fixes

- **separator**: CoreML session options and macOS regression test
- **ui**: Portal song dialogs above list rows

## [0.5.0] - 2026-04-12

### ♻️ Refactoring

- Reduce codebase entropy across four axes
- Rename LyricsMatch.song_hash to song_id for IPC consistency
- Split import.rs into directory module with focused sub-modules
- **lyrics**: Extract three hooks from LyricsPanel

### ✨ New Features

- **separator**: Add ExecutionProviderPreference to config.rs
- **separator**: Add coreml and directml features to ort dependency
- **separator**: Add EP selection and graceful fallback to model loading
- **separator**: Thread EP preference through separation pipeline
- **separator**: Add execution provider to AppSettings and settings command
- **separator**: Add execution provider to frontend types, API, and state
- **separator**: Add hardware acceleration settings UI and i18n
- Relocate sidebar toggle and import buttons to the left section of the desktop titlebar

### 🐛 Bug Fixes

- Use ubuntu-22.04 for GLIBC compatibility
- Use developer_name instead of developer tag, remove vcs-browser URL for appstream compatibility on Ubuntu 22.04
- Use tauri-plugin-dialog 2.1.0 (2.6.0 not available)
- Use tauri-plugin-dialog 2.7.0 (latest compatible version)
- **ci**: Add OS-specific cache key and sync Cargo.lock
- **ci**: Properly update Cargo dependencies via cargo update
- Remove unused import and dead-code warnings in window_shell.rs
- **ci**: Exclude weak symbols from glibc floor check
- **ci**: Improve weak symbol filter and add diagnostic output
- **ci**: Use portable awk pattern for weak symbol detection
- **ci**: Correct ORT library path case in glibc floor check
- **macos**: Disable movableByWindowBackground for correct drag regions

### 📝 Documentation

- Add Cursor Cloud specific instructions to AGENTS.md
- Update architecture.md to reflect implemented hardware acceleration

### 📦 Dependencies

- Bump Actions to Node 24 runtimes

## [0.4.0] - 2026-04-09

### ♻️ Refactoring

- Refine native macOS window shell integration and simplify UI element styling.
- Simplify Sidebar and LyricsPanel tests by removing unused props and cleaning up expectations

### ✨ New Features

- Add demo video, app screenshot, and import GIF sections to the website with new styling and assets.
- Enhance macOS support with improved AirPlay integration and UI updates
- **macos**: Add native shell runtime and settings sync
- **macos**: Align native shell visuals and controls
- Enhance macOS native shell support with new sidebar header height and layout adjustments
- Unify app shell structure and remove secondary webview components

### 🐛 Bug Fixes

- **release**: Soften winget pr automation and update release docs
- **ui**: Restore stable shell chrome layout

### 📝 Documentation

- Update v0.4.0 release references

### 🔧 Chores

- Add GitHub issue template

### 🧪 Tests

- Enhance NativeFloatingControls and AirPlayRouteButton tests with new utility button footprint and resize observer functionality

## [0.3.0] - 2026-03-22

### 🐛 Bug Fixes

- Install pnpm in packaging workflows
- Add winget schema headers
- Align winget manifests with validator schema
- Match winget default locale header casing
- Handle hardlinked ORT DLLs in Windows CI copy step
- **ci**: Stabilize windows ort staging and clean rust warnings
- Stage ORT runtime DLLs for Windows CI tests and conditionally import `Emitter` in `airplay.rs`.
- Update Windows CI to correctly stage ORT runtime DLLs for Rust tests by reading the `dfbin` cache path from `ort-sys` build output.
- Download and stage DirectML.dll v1.15.4 for Windows CI and refine ort-sys DLL staging logic.

### 📝 Documentation

- Add product demo video to README and README_CN
- Add detailed explanation for Windows `cargo test` skip in CI workflow and AGENTS.md.
- Mark v0.3.0 as released

### 📦 Dependencies

- Update Windows runner to 2022, add DLL dependency diagnosis, and ensure Rust tests find staged DLLs on PATH.
- Enhance Windows DLL dependency diagnosis in CI by adding `continue-on-error`, switching to `link.exe /dump /dependents`, and implementing error handling for dependency checks.
- Update Windows runner to `windows-latest` and refine DLL staging for Rust tests to explicitly provide OnnxRuntime and DirectX 12 dependencies.
- Skip Rust tests on Windows runners and remove the associated ORT/DirectML DLL staging due to runtime dependency issues.

### 🔧 Chores

- Upgrade batch 1 dependencies
- Upgrade batch 2 dependencies

## [0.2.0] - 2026-03-19

### ♻️ Refactoring

- Extract services layer and slim down monolithic modules

### ✨ New Features

- Add dual model support (htdemucs + htdemucs_ft)
- Add Liquid Glass icon support for macOS 26

### 🐛 Bug Fixes

- Restore model bootstrap and CDG playback
- CDG rendering in main window, fullscreen mirroring, and binary IPC
- Stabilize playback display overlays and plain-text lyrics
- Restore CDG playback and disambiguate sidecar pairing
- Sync fullscreen CDG and simplify media-g imports
- Close fullscreen window and drop stale CDG frames
- Resolve secondary window CDG layout and sync performance issues
- Fix fullscreen CDG sync lag and close button, improve lyrics audience layout
- Remove invalid core:webview:allow-close permission
- Replace rAF with setTimeout(0) for fullscreen CDG painting
- Use native window for audience display, add CDG frame versioning
- Remove invalid extendInfo from tauri.conf.json
- Make release workflow dispatchable

### 📝 Documentation

- Add Homebrew install command and .deb to platform table
- Add macOS quarantine removal command to install instructions
- Reorganize repository docs
- Document CD+G library support

## [0.1.0] - 2026-03-16

### ♻️ Refactoring

- Migrate model source to openkara-models repository

### ⚡ Performance

- Add backend performance baseline

### ✨ New Features

- Add metadata parsing module
- Add songs sqlite cache
- Add import songs command
- Add audio decode pipeline
- Add playback state machine and events
- Add cpal playback output
- Add separation model loader
- Add separation preprocess pipeline
- Add separation inference pipeline
- Add accompaniment mixing
- Add stems cache
- Add background separation worker
- Add karaoke playback mode
- Add lyrics fetch pipeline
- Add lyrics cache and commands
- Add structured command errors
- Structure import failure errors
- Add first-run model bootstrap
- Add local audio smoke harness
- Normalize and chunk demucs inputs
- Implement full karaoke frontend UI
- Implement portable karaoke library system
- Per-stem volume controls with collapsible accompaniment mixer
- Separation pipeline optimizations (resumability, dual modes, compression)
- Lyrics system, song properties, volume controls, and 4-stem UI
- Playback queue system with auto-advance and song transitions
- Library operations — batch separation, bulk delete, and embedded lyrics
- Add full i18n support with English and Simplified Chinese
- Network lyrics, smart batch button, fix 4-stem upgrade, stem downgrade

### 🐛 Bug Fixes

- Set mainBinaryName so Tauri bundles the app entry instead of the smoke-test CLI
- Replace HTML file input with Tauri dialog plugin for audio import
- Update model SHA-256 and release tag to model-v1.0.0
- Resolve 15+ real-world bugs and add song metadata editing
- Player bugs (seek, volume, pause, crackling) and UI layout overhaul
- Eliminate audio crackling by using frame counter instead of wall clock
- Hydrate separation statuses from database on app startup
- UI/UX improvements and bug fixes for lyrics, separation, and player
- Resolve 5 ESLint errors breaking CI lint step
- Add missing eslint dep, unused var, empty catch + prettier formatting
- Normalize line endings for Prettier in CI
- Resolve play/pause position reset, improve UI and onboarding
- Migrate queue reordering to dnd-kit
- Refine queue drag feedback and accessibility
- Harden editing flows and refine setup copy
- Avoid playback stutter during track switches
- Stabilize queue drag animations

### 📝 Documentation

- Add bilingual project readmes
- Add architecture, project structure, and directory skeleton
- Add development phases, technical roadmap, and milestones
- Add local setup and handoff instructions
- Add execution handoff plans
- Record phase 1 library contract
- Record playback contract
- Record separation contract
- Update playback contract for karaoke mode
- Record lyrics contract
- Record structured error contract
- Update library error contract
- Add phase 5 performance baseline
- Record model bootstrap contract
- Expand install and release instructions
- Sync execution plans with current progress
- Record model distribution strategy
- Switch release plan to homebrew cask
- Rewrite README as mature open-source project with MIT license
- Update README, milestones, and roadmap to reflect current project state
- Add OpenKara SVG logo asset
- Add openkara-models AI model info to both READMEs
- Add GitHub Pages microsite and reorganize docs
- Migrate Pages site to Jekyll
- Rewrite site copy, add dark theme, add deb target, update READMEs
- Codify agent verification and format site files

### 📦 Dependencies

- Add cross-platform foundation workflow
- Finalize tauri app configuration
- Align verification with model setup
- Add release workflow
- Scaffold homebrew cask packaging
- Fix Linux verify dependencies and update actions
- Add custom release notes input to release workflow
- Pin tauri-action to action-v0.6.2
- Add release checksums, Intel Mac dylib bundling, update demucs links
- Use ONNX Runtime 1.20.1 for Intel Mac (last x86_64 prebuilt)

### 🔧 Chores

- Add gitignore
- Ignore local worktrees
- Bootstrap tauri app shell
- Add frontend foundation tooling
- Add sqlite migration foundation
- Add model setup bootstrap script
- Align formatting for repo tooling
- Ignore generated files in prettier
- Disable doctests for tauri crate
- Add branded app icon assets and docs
- Remove local test audio from repository
- Remove packaging/homebrew and render-homebrew-cask script

### 🧪 Tests

- Add backend karaoke flow smoke test
- Stabilize temp fixtures for parallel runs
