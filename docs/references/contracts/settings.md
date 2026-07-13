# Settings IPC Contract

Settings commands manage persistent application preferences stored in
`AppConfig`. Each setter returns the full `AppSettings` snapshot so the
frontend can rehydrate its store in one round-trip.

## Commands

### `set_theme_preference(preference: ThemePreference) -> AppSettings`

Updates the persisted appearance preference and returns the refreshed
`AppSettings` snapshot.

**Parameter:**

- `preference: ThemePreference` — one of `"system"`, `"light"`, `"dark"`.

**Return:**

- `AppSettings` — the full settings snapshot with `theme_preference` updated.

**Frontend wrapper:** `src/lib/tauri/settings.ts` → `setThemePreference(preference)`

**Rust handler:** `src-tauri/src/commands/settings.rs` → `set_theme_preference`

**Capability:** `core:window:allow-set-theme` (required for the native window
theme sync that follows a preference change).

## ThemePreference Enum

```rust
pub enum ThemePreference {
    System,
    Light,
    Dark,
}
```

Serialized as lowercase strings: `"system"`, `"light"`, `"dark"`.

The frontend `ThemePreference` type mirrors this in `src/types/ipc.ts`.

## Effective Theme Resolution

`AppConfig::effective_theme_preference()` resolves `System` against the OS
appearance at call time, returning `Light` or `Dark`. The frontend
`theme-runtime.ts` performs the same resolution via `matchMedia` and applies
the `data-theme` attribute to the document root.

## AppSettings Snapshot

The `AppSettings` struct returned by every settings command includes:

| Field                           | Type                  | Notes                                         |
| ------------------------------- | --------------------- | --------------------------------------------- |
| `stem_mode`                     | `StemMode`            | `"two_stem"` or `"four_stem"`                 |
| `model_variant`                 | `ModelVariant`        | `"htdemucs"` or `"htdemucs_ft"`               |
| `language`                      | `string \| null`      | BCP-47 tag; `null` means "use system default" |
| `hide_batch_separate`           | `bool`                |                                               |
| `cover_art_backdrop`            | `bool`                |                                               |
| `lyrics_font_step`              | `i8`                  |                                               |
| `execution_provider`            | `ExecutionProvider`   | `"cpu"` or `"xnnpack"`                        |
| `available_execution_providers` | `ExecutionProvider[]` |                                               |
| `theme_preference`              | `ThemePreference`     | Added in #96                                  |
