# Translating OpenKara

OpenKara uses [react-i18next](https://react.i18next.com/) with static JSON translation files. English (`en.json`) is the source of truth. We welcome contributions for new languages.

Locale files are **auto-loaded**: `src/lib/i18n.ts` globs `src/locales/*.json` at build time (via `import.meta.glob`), so dropping a new file into `src/locales/` registers the language everywhere (i18next resources, onboarding picker, settings dropdown, system-language detection). The only companion edit is a one-line native name.

## Adding a new language

1. **Copy the English file**

   ```
   cp src/locales/en.json src/locales/{code}.json
   ```

   Use a BCP 47 language code for the filename (see [Language codes](#language-codes) below), e.g. `ja.json`, `pt-BR.json`.

2. **Translate all values**

   Open your new file and translate every JSON **value**. Do **not** change, add, or remove keys (except plural categories — see [Plurals](#plurals)). Keep `{{placeholders}}` exactly as they are.

3. **Add the native name to the map in `src/lib/i18n.ts`**

   Add one entry to `NATIVE_LANGUAGE_NAMES`, keyed by your file's code, using the language's own native name:

   ```ts
   export const NATIVE_LANGUAGE_NAMES: Record<string, string> = {
     en: "English",
     "zh-CN": "简体中文",
     ja: "日本語", // <- add this
   };
   ```

   The canonical native name for every planned language is in the
   [Native names](#native-names) table below — copy it verbatim so names never
   drift. That is the **only** code change needed; you do **not** touch the
   `i18next.init` resources or `SUPPORTED_LANGUAGES` (both are derived
   automatically). Display order in the pickers follows `LANGUAGE_PRIORITY` in
   the same file.

4. **Run the checks**

   ```
   node scripts/check-i18n.mjs      # key structure + plural categories
   pnpm vitest run src/locales      # parses, placeholders, non-empty values
   pnpm vitest run src/lib/i18n     # system-language detection
   ```

   All must pass. `check-i18n` prints a per-file report; fix every `MISSING`
   and `EXTRA` line (`WARN` lines about extra plural categories are tolerated).

5. **Eyeball it**

   Run the app and switch to your language in **Settings → Language** (and on the
   first-run onboarding step). Check the four dense surfaces: onboarding, the
   settings panel, the playback bar, and the lyrics panel. Any English text
   showing through in a non-English UI is a bug.

## Plurals

Count-aware strings use i18next's plural suffix convention: a base key plus an
underscore and an Intl plural **category**:

```
_zero  _one  _two  _few  _many  _other
```

`en.json` has one plural base key today:

- `library.confirmDeleteTitle` → `library.confirmDeleteTitle_one`, `library.confirmDeleteTitle_other`

**Provide exactly the categories your language uses**, no more and no less. The
required set is `new Intl.PluralRules("{code}").resolvedOptions().pluralCategories`:

| Category set required         | Languages (this round)           |
| ----------------------------- | -------------------------------- |
| `other`                       | zh-CN, zh-TW, ja, ko, th, vi, id |
| `one`, `other`                | en, de, nl, tr                   |
| `one`, `many`, `other`        | es, pt-BR, fr, it                |
| `one`, `few`, `many`, `other` | ru, pl                           |

So a `library.confirmDeleteTitle` translation is `…_other` only for Japanese,
but `…_one` / `…_few` / `…_many` / `…_other` for Russian. `check-i18n.mjs` fails
if a required category is missing; a category **beyond** the Intl set is only a
warning (it is simply never selected).

> The suffix must be a literal `_category`. A key that merely ends in the word
> "Other"/"One" (e.g. `songProperties.channelsOther`, which is hand-pluralized
> in code) is **not** a plural key — translate it as an ordinary value.

## Translation guidelines

- **Keys are sacred.** Never rename, add, or remove keys. Only translate the values (plural categories per your language are the sole exception).
- **Keep `{{variable}}` placeholders as-is.** The system injects these dynamic values at runtime (e.g., `"Separating {{current}}/{{total}}"`). Translate the surrounding text; leave the placeholders untouched. A dropped placeholder fails `pnpm vitest run src/locales`.
- **Keep labels short.** Button, menu, tab, and toolbar strings are laid out in tight space — match English length where you can.
- **Preserve special characters.** Keep unicode escapes like `…` (ellipsis) or replace them with the actual character (`…`).
- **Don't translate brand/format names.** Keep "OpenKara", "ONNX Runtime", "LRC", "TTML", "Lyricify", "CD+G", and model names ("htdemucs", …) unchanged.
- **Use the glossary.** Render the [terminology](#terminology-glossary) below consistently across all of your strings.
- **Match the tone.** OpenKara's UI is concise and direct. Avoid overly formal or verbose translations.

## Terminology glossary

These are OpenKara's recurring domain terms with their **meaning**. The glossary
defines the concept; you choose the best, most natural term in your language —
just keep it consistent everywhere it appears.

| Term                               | Meaning in OpenKara                                                                                                              |
| ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| **stem**                           | One isolated audio track produced by AI source separation (vocals, drums, bass, or other).                                       |
| **separation**                     | The AI process that splits a song into stems for karaoke.                                                                        |
| **stem mode (2-stem/4-stem)**      | How many stems a separation outputs: 2 = vocals + accompaniment; 4 = vocals + drums + bass + other.                              |
| **vocals**                         | The isolated singing/voice stem (the part removed for karaoke).                                                                  |
| **accompaniment**                  | The instrumental backing left after vocals are removed — the karaoke track. In 4-stem it is drums + bass + other mixed together. |
| **instrumental**                   | A song that has no vocals / is marked as vocals-free backing.                                                                    |
| **downgrade / compress**           | Convert a 4-stem separation down to 2-stem (merge drums/bass/other into one accompaniment) to save disk space.                   |
| **library**                        | The user's local collection of imported songs.                                                                                   |
| **repository / remote repository** | A remote storage backend (cloud drive or WebDAV server) that stores/mirrors the library. NOT a code/git repository.              |
| **remote**                         | Belonging to the remote repository / cloud, as opposed to local.                                                                 |
| **mirror**                         | To keep a copy of the local library synchronized up to a remote repository.                                                      |
| **queue**                          | The ordered list of songs to play next ("Up Next").                                                                              |
| **rotation**                       | The karaoke singer rotation — cycling turns among singers so each gets to sing.                                                  |
| **singer**                         | A named participant in the rotation to whom queued songs are assigned.                                                           |
| **playback bar**                   | The bottom transport bar: play/pause, previous/next, seek, volume.                                                               |
| **lyrics offset**                  | A time shift (± seconds) applied to lyric timing to sync captions with the audio.                                                |
| **romanization / romanized**       | Transliteration of non-Latin lyrics into the Latin alphabet (e.g. Japanese → romaji) for singing along.                          |
| **equalizer (EQ)**                 | The multi-band frequency tone control for playback.                                                                              |
| **band**                           | One adjustable frequency range of the equalizer (e.g. 60 Hz, 3.6 kHz).                                                           |
| **preset**                         | A named, saved equalizer configuration (Flat, Vocal Boost, …).                                                                   |
| **cache**                          | Locally stored data kept to avoid recomputing or re-downloading — cached stems, cached lyrics, or remote-media byte-range cache. |
| **runtime (ONNX Runtime)**         | The library that executes the AI separation model. Keep the name "ONNX Runtime".                                                 |
| **model / model variant**          | The AI separation model and its variants (e.g. htdemucs); keep model names untranslated.                                         |
| **execution provider**             | The compute backend the model runs on (CPU / GPU / CoreML, etc.).                                                                |

## File structure

Translation files use a flat namespace of feature areas. The table below shows what each section covers:

| Namespace        | Description                                       |
| ---------------- | ------------------------------------------------- |
| `app`            | App-level chrome and window title bits            |
| `common`         | Shared labels: Cancel, Save, Close, Search, etc.  |
| `setup`          | First-run library/language/stem/model onboarding  |
| `sidebar`        | Sidebar navigation and batch separation           |
| `toolbar`        | Top toolbar actions                               |
| `windowChrome`   | Window controls (minimize/maximize/close)         |
| `library`        | Track list, context menu, lyrics-language picker  |
| `player`         | Playback controls (play, pause, seek, volume)     |
| `progress`       | Global progress toasts/labels                     |
| `queue`          | Play queue panel (incl. drag-reorder a11y copy)   |
| `stems`          | Stem mixer (vocals, drums, bass, other)           |
| `lyrics`         | Lyrics display, editing, offset, romanization     |
| `songEdit`       | Song metadata editing dialog                      |
| `songProperties` | Song properties/info dialog                       |
| `settings`       | Preferences panel (EQ, cache, danger zone, about) |
| `bootstrap`      | AI model download and setup banners               |
| `playlist`       | Playlists                                         |
| `rotation`       | Karaoke singer rotation                           |
| `errors`         | Error messages and titles                         |

## Native names

Copy the native name verbatim into `NATIVE_LANGUAGE_NAMES` when you add a file.

| Code    | Native name        |
| ------- | ------------------ |
| `en`    | English            |
| `zh-CN` | 简体中文           |
| `ja`    | 日本語             |
| `ko`    | 한국어             |
| `zh-TW` | 繁體中文           |
| `es`    | Español            |
| `pt-BR` | Português (Brasil) |
| `fr`    | Français           |
| `de`    | Deutsch            |
| `it`    | Italiano           |
| `ru`    | Русский            |
| `id`    | Bahasa Indonesia   |
| `vi`    | Tiếng Việt         |
| `th`    | ไทย                |
| `tr`    | Türkçe             |
| `pl`    | Polski             |
| `nl`    | Nederlands         |

## Language codes

Use [BCP 47](https://www.rfc-editor.org/info/bcp47) language tags. Use the shortest code that uniquely identifies the language; add a region subtag only to distinguish variants (e.g., `pt-BR` vs `pt-PT`, `zh-CN` vs `zh-TW`).

System-language detection (`detectSystemLanguage`) resolves a browser/OS locale to the closest shipped file: an exact tag first, then Chinese by script/region (Traditional → `zh-TW`, otherwise `zh-CN`), Portuguese → `pt-BR`, and finally the base tag (e.g. `de-AT` → `de`), falling back to English.
