# Settings Contract

Field or command semantic changes must update this document before changing UI code.

## AppSettings payload

`get_settings() -> AppSettings`

| Field | Type | Notes |
| ------------------------------- | ------------- | --------------------------------------------------- | ----------------------------------------- |
| `stem_mode` | `"two_stem"   | "four_stem"` | Active stem separation mode |
| `model_variant` | `"htdemucs"   | "htdemucs_ft"` | Active Demucs model variant |
| `language` | `string       | null` | UI language code, `null` = system default |
| `hide_batch_separate` | `boolean` | Hide batch-separate action in UI |
| `cover_art_backdrop` | `boolean` | Show blurred cover-art backdrop in player |
| `hide_upgrade_all` | `boolean` | Hide "Upgrade All to 4-stem" action in UI |
| `lyrics_font_step` | `i8` | Range `[-2, 2]`, 0 = default |
| `execution_provider` | `string` | Active ONNX Runtime execution provider |
| `available_execution_providers` | `Vec<string>` | Providers available on current platform |
| `eq_enabled` | `bool` | Whether the five-band equalizer is enabled |
| `eq_gains_db` | `[f32; 5]` | Per-band dB gains, each clamped to `[-12, 12]` |
| `crossfade_enabled` | `bool` | Whether equal-power crossfade is enabled |
| `crossfade_duration_ms` | `u32` | Crossfade duration in ms, clamped to `[500, 10000]` |
| `library_sort_mode` | `string` | Active sidebar song-list sort mode (see below) |
| `theme_preference` | `string` | Appearance preference (see below) |

## Library sort mode

`LibrarySortMode` enum values (serialized as lowercase snake_case strings):

| Value                 | Meaning                                    |
| --------------------- | ------------------------------------------ |
| `"recently_imported"` | Sort by `imported_at` descending (default) |
| `"title_asc"`         | Sort by title ascending                    |
| `"artist_asc"`        | Sort by artist ascending                   |

For the two alphabetical modes, the primary order uses the A–Z / `#` alphabet
rail bucket. Han-leading text uses its pinyin initial. Locale collation orders
entries within a bucket. This keeps every rail bucket contiguous. The rail's
target indices increase monotonically from A through `#`.

### Commands

- `set_library_sort_mode(mode: LibrarySortMode) -> AppSettings` — Persist the
  library sort mode. Return the updated settings snapshot.

## Theme preference

`ThemePreference` enum values (serialized as lowercase snake_case strings):

| Value      | Meaning                               |
| ---------- | ------------------------------------- |
| `"system"` | Follow OS appearance                  |
| `"light"`  | Force light theme                     |
| `"dark"`   | Force dark theme (default when unset) |

`AppConfig::effective_theme_preference()` returns the persisted preference. It
defaults to `Dark` when unset. It does **not** resolve `System` against the OS
appearance. The frontend `theme-runtime.ts` resolves `System` to `Light` or
`Dark` with `matchMedia("(prefers-color-scheme: dark)")`. It applies the
`data-theme` attribute to the document root. The audience/fullscreen stage
stays explicitly dark regardless of the primary preference.

### Commands

- `set_theme_preference(preference: ThemePreference) -> AppSettings` — Persist
  the appearance preference. Return the updated settings snapshot.
  Frontend wrapper: `setThemePreference`. Native window theme sync requires
  the `core:window:allow-set-theme` capability.

## Other settings commands

- `set_stem_mode(mode: String) -> AppSettings`
- `set_model_variant(variant: String) -> AppSettings`
- `set_language(language: String) -> AppSettings`
- `set_hide_batch_separate(value: bool) -> AppSettings`
- `set_cover_art_backdrop(value: bool) -> AppSettings`
- `set_hide_upgrade_all(value: bool) -> AppSettings`
- `set_lyrics_font_step(step: i8) -> AppSettings`
- `set_execution_provider(provider: String) -> AppSettings`
- `set_eq_enabled(enabled: bool) -> AppSettings`
- `set_eq_gains(gains_db: [f32; 5]) -> AppSettings`
- `set_crossfade_enabled(enabled: bool) -> AppSettings`
- `set_crossfade_duration_ms(duration_ms: u32) -> AppSettings`
- `set_update_policy(policy: UpdatePolicy) -> AppSettings`
- `restart_app() -> ()`

## Update policy

`UpdatePolicy` enum values (serialized as lowercase snake_case strings):

| Value             | Meaning                                                               |
| ----------------- | --------------------------------------------------------------------- |
| `"manual"`        | Only check for updates when the user triggers it manually             |
| `"notify"`        | Check on startup and surface available updates (default)              |
| `"auto_download"` | Download updates in the background; activation still requires restart |

### Command

- `set_update_policy(policy: UpdatePolicy) -> AppSettings` — Persist the
  update policy. Return the updated settings snapshot. Activation of a
  staged runtime always requires a restart regardless of policy; the policy
  only governs checking and downloading.

## Debug info

`get_debug_info() -> DebugInfo`

Returns a diagnostic snapshot for bug reports. Both the Settings → About
panel and the macOS Help menu call this command. The frontend owns the
plain-text formatting.

| Field                     | Type             | Notes                                                   |
| ------------------------- | ---------------- | ------------------------------------------------------- |
| `app_version`             | `string`         | Application version from the bundle                     |
| `build_sha`               | `string`         | Git short SHA, or `"unknown"`                           |
| `os`                      | `string`         | Operating system family (`macos` / `windows` / `linux`) |
| `arch`                    | `string`         | CPU architecture (`aarch64` / `x86_64`)                 |
| `catalog_generation`      | `u64`            | Embedded catalog generation                             |
| `catalog_release_id`      | `string`         | Embedded catalog release ID                             |
| `model_variant`           | `string`         | Active separation model variant                         |
| `model_state`             | `string`         | Bootstrap state of the active model                     |
| `model_installed`         | `bool`           | Whether the active model is installed and verified      |
| `model_installed_version` | `Option<string>` | Installed model upstream tag, when known                |
| `model_pinned_version`    | `string`         | Upstream tag pinned by the embedded catalog             |
| `model_path`              | `string`         | Expected model file path on disk                        |
| `runtime_state`           | `string`         | Bootstrap state of the ONNX Runtime                     |
| `runtime_version`         | `string`         | Active runtime upstream version                         |
| `runtime_artifact_id`     | `Option<string>` | Active runtime artifact ID                              |
| `runtime_target_triple`   | `string`         | Runtime target triple for this build                    |
| `execution_provider`      | `string`         | Current execution-provider preference                   |
| `log_file`                | `string`         | Path pattern of the rolling log file                    |

## Self-update probe

`self_update_supported() -> bool`

Returns whether the running install can self-update through
`tauri-plugin-updater`. Only bundles that emit signed updater artifacts are
updatable: the AppImage on Linux, the `.app`/DMG on macOS, and the NSIS
installer on Windows. A Linux `.deb`, Flatpak, or an unbundled dev binary
returns `false`. The frontend gates its launch update check on this probe.

## Remote streaming cache

The remote streaming cache stores byte-range downloads of remote media files
so playback can resume without re-fetching. The cache is content-addressed by
the SHA-256 of `(library_id, relative_path, provider_revision,
expected_size)`, so a replaced remote object (new revision or size) does not
reuse bytes from an older version. The durable catalog lives in the local-only
`remote_cache_entries` table (`remote-state.db`); on-disk data files live in
`<app-data>/remote-cache/`.

When `remote_cache_bytes_limit` is unset, the cache defaults to a finite 2 GiB
budget. The configured limit is read at startup from `AppConfig`.

### Commands

- `get_remote_cache_usage() -> CacheUsage` — Report cache usage.
  - `used_bytes`: total bytes used by reconciled catalog entries.
  - `limit_bytes`: the configured byte budget (2 GiB default).
  - `entry_count`: number of catalog entries.
  - `pinned_count`: number of entries currently pinned by active playback
    (exempt from eviction).
- `clear_remote_cache() -> usize` — Evict all unpinned cache entries. Pinned
  entries (files in active use by playback) remain until playback releases
  them, then a subsequent clear or LRU eviction removes them. Returns the
  number of entries evicted.

## Remote diagnostics

`get_remote_diagnostics() -> RemoteDiagnostics`

Returns a diagnostic snapshot of the remote repository state for the active
remote library. When no remote library is active, returns a snapshot with
`has_active_remote: false` and all other fields zeroed/empty.

| Field                   | Type                             | Notes                                                  |
| ----------------------- | -------------------------------- | ------------------------------------------------------ |
| `has_active_remote`     | `bool`                           | `true` when a remote library is active                 |
| `repository_id`         | `string \| null`                 | Stable repository UUID (manifest protocol)             |
| `writer_id`             | `string \| null`                 | Stable installation UUID (diagnostics only)            |
| `committed_generation`  | `i64`                            | Monotonically increasing remote generation             |
| `local_base_generation` | `i64`                            | Generation the local working copy was last synced from |
| `local_state`           | `string`                         | `clean`, `dirty`, or `conflicted`                      |
| `local_db_digest`       | `string \| null`                 | SHA-256 of the local working database                  |
| `active_operation_id`   | `string \| null`                 | Active publish/GC operation ID                         |
| `last_success_at_ms`    | `i64 \| null`                    | Wall-clock ms of last successful publication           |
| `last_error_code`       | `string \| null`                 | Last error code (e.g. `remote_conflict`)               |
| `recent_operations`     | `Vec<RemoteOperationDiagnostic>` | Most recent 20 operations (newest first)               |

`RemoteOperationDiagnostic` fields: `operation_id`, `operation_kind`
(`publish` / `gc`), `state` (`pending` / `running` / `completed` / `failed` /
`conflicted` / `cancelled`), `expected_generation`, `target_generation`,
`attempt_count`, `error_code`, `error_detail`, `created_at_ms`,
`updated_at_ms`.
