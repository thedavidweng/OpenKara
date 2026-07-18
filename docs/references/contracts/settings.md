# Settings Contract

Field or command semantic changes must update this document before changing UI code.

## AppSettings payload

`get_settings() -> AppSettings`

| Field                           | Type                          | Notes                                          |
| ------------------------------- | ----------------------------- | ---------------------------------------------- |
| `stem_mode`                     | `"two_stem" \| "four_stem"`   | Active stem separation mode                    |
| `model_variant`                 | `"htdemucs" \| "htdemucs_ft"` | Active Demucs model variant                    |
| `language`                      | `string \| null`              | UI language code, `null` = system default      |
| `hide_batch_separate`           | `boolean`                     | Hide batch-separate action in UI               |
| `cover_art_backdrop`            | `boolean`                     | Show blurred cover-art backdrop in player      |
| `lyrics_font_step`              | `i8`                          | Range `[-2, 2]`, 0 = default                   |
| `execution_provider`            | `string`                      | Active ONNX Runtime execution provider         |
| `available_execution_providers` | `Vec<string>`                 | Providers available on current platform        |
| `eq_enabled`                    | `bool`                        | Whether the five-band equalizer is enabled     |
| `eq_gains_db`                   | `[f32; 5]`                    | Per-band dB gains, each clamped to `[-12, 12]` |
| `library_sort_mode`             | `string`                      | Active sidebar song-list sort mode (see below) |

## Library sort mode

`LibrarySortMode` enum values (serialized as lowercase snake_case strings):

| Value                 | Meaning                                    |
| --------------------- | ------------------------------------------ |
| `"recently_imported"` | Sort by `imported_at` descending (default) |
| `"title_asc"`         | Sort by title ascending                    |
| `"artist_asc"`        | Sort by artist ascending                   |

For the two alphabetical modes, the primary order is the A–Z / `#` alphabet
rail bucket. Han-leading text uses its pinyin initial; locale collation orders
entries within a bucket. This keeps every rail bucket contiguous and makes the
rail's target indices monotonically increase from A through `#`.

### Commands

- `set_library_sort_mode(mode: LibrarySortMode) -> AppSettings` — Persist the
  library sort mode and return the updated settings snapshot.

## Other settings commands

- `set_stem_mode(mode: String) -> AppSettings`
- `set_model_variant(variant: String) -> AppSettings`
- `set_language(language: String) -> AppSettings`
- `set_hide_batch_separate(value: bool) -> AppSettings`
- `set_cover_art_backdrop(value: bool) -> AppSettings`
- `set_lyrics_font_step(step: i8) -> AppSettings`
- `set_execution_provider(provider: String) -> AppSettings`
- `set_eq_enabled(enabled: bool) -> AppSettings`
- `set_eq_gains(gains_db: [f32; 5]) -> AppSettings`
- `restart_app() -> ()`
