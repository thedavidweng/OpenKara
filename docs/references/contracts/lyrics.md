# 歌词契约

覆盖歌词 IPC：AMLL client、LRCLIB client、LrcApi client、LRC/TTML/LYS parser、抓取优先链、SQLite cache，以及 `fetch_lyrics / fetch_lyrics_online / set_lyrics_offset / import_lyrics_files` 命令。

## 接口

1. `fetch_lyrics(song_id: String) -> LyricsPayload`
2. `set_lyrics_offset(song_id: String, ms: i64) -> ()`
3. `set_lyrics_font_step(step: i8) -> AppSettings`
4. `save_manual_lyrics(song_id: String, text: String) -> LyricsPayload`
5. 抓取优先顺序固定为 `cache -> embedded -> sidecar TTML -> sidecar LYS -> sidecar LRC -> AMLL -> LRCLIB -> LrcApi`
6. sidecar 优先级固定为 `.ttml -> .lys -> .lrc`；每个候选格式必须先能解析出至少一行歌词，格式错误时继续尝试下一种
7. SQLite `lyrics` 表按 `song_hash` 缓存原始歌词文本和 `offset_ms`
8. 对同一首歌重复调用 `fetch_lyrics` 时，优先命中 SQLite cache，不重复发起 HTTP 请求
9. 歌词字号是全局显示偏好，不写入 `lyrics` 表；它走 `AppSettings.lyrics_font_step`
10. 歌词命令失败值统一为 `CommandError`，详见 [errors.md](./errors.md)

## Inputs / outputs / required dependencies

### Command: `fetch_lyrics`

**Input**

```json
{
  "song_id": "sha256 hash string"
}
```

**Output**

```json
{
  "song_id": "sha256 hash string",
  "lines": [
    {
      "time_ms": 35660,
      "text": "Look at the stars",
      "words": [
        {
          "time_ms": 35660,
          "end_ms": 36020,
          "text": "Look",
          "roman": null
        }
      ],
      "bg_words": null,
      "section": null,
      "roman": null
    }
  ],
  "source": "lrc_lib",
  "offset_ms": 0,
  "raw_lrc": "[00:35.66]<00:35.66>Look <00:36.02>at the stars"
}
```

**Miss output**

```json
{
  "song_id": "sha256 hash string",
  "lines": [],
  "source": null,
  "offset_ms": 0,
  "raw_lrc": ""
}
```

**Semantics**

1. `song_id` 对应 `songs.hash`
2. 后端会先检查 SQLite `lyrics` cache；命中后直接用 `parse_lyrics_auto` 解析缓存的原始歌词文本
3. cache miss 时，后端按固定顺序尝试：
   - 音频文件内嵌歌词标签
   - 同名 sidecar `.ttml`
   - 同名 sidecar `.lys`
   - 同名 sidecar `.lrc`
   - AMLL `GET /v1/lyrics/search` then `GET /v1/lyrics/get`；仅在自信匹配且 TTML 含 word tokens 时命中
   - LRCLIB `GET /api/get`
   - LrcApi `GET /jsonapi`；优先用 `lrc`，没有 synced LRC 时可用 `lrc_ttml`
4. 一旦抓到歌词，后端会先解析成 `Vec<LyricLine>`，再把原始歌词文本、来源和 `offset_ms = offset_ms_for_raw(raw)` 写入 SQLite（LRC `[offset:]`，可选 TTML 声明 offset，否则 0）。命令返回值从 cache 行重建，因此 payload 的 `offset_ms` 与磁盘一致。
5. 在线 provider 的请求失败不会被当作确定缺失；后端会继续尝试后续 provider，并且不会写入 negative cache
6. 如果所有来源都 miss，命令仍然成功返回；只是 `lines = []`、`source = null`
7. 如果歌曲不存在、文件读取失败或歌词解析失败，命令返回 `CommandError`

### Command: `fetch_lyrics_online`

**Input**

```json
{
  "song_id": "sha256 hash string",
  "intent": "automatic_upgrade"
}
```

**Semantics**

1. 仅尝试在线 timed lyrics provider，不读取 embedded 或 sidecar。Provider 集合取决于 `intent` 和当前 cache 行（见下表）
2. `intent` 必须为 `automatic_upgrade` 或 `user_replace`
3. 完整在线链顺序固定为 `AMLL -> LRCLIB -> LrcApi`。Word-timed Upgrade 路径（`automatic_upgrade` 且当前行为 `lrc_lib` / `lrc_api` / `lrc_api_ttml`）只调用 AMLL
4. `user_replace` 只有在完整链中全部 provider 都确定缺失时，才写入 7 天 `absent` 负缓存并返回空 payload。Word-timed Upgrade miss 返回当前缓存 payload，不得写入 `absent`，改为盖 `word_timed_checked_at` 戳
5. `automatic_upgrade` 可以用任意 timed 在线结果替换 `embedded` 或 `absent`。它只能用 word-timed `amll` 替换 `lrc_lib` / `lrc_api` / `lrc_api_ttml`。`user_replace` 可以替换任何现有行
6. 如果任一 provider 命中，返回的 `LyricsPayload` 与 `fetch_lyrics` 保持一致，并将结果写入 SQLite cache
7. 如果所有在线 provider 都因为请求或响应错误而无法返回 timed lyrics，命令返回 `CommandError`，不写入 negative cache。429 / 5xx / timeout 不盖 probe 戳

| Intent                                                        | Online providers       | May replace             |
| ------------------------------------------------------------- | ---------------------- | ----------------------- |
| `automatic_upgrade` + current Online Lyrics Source Line-timed | AMLL only              | Only Word-timed `amll`  |
| `automatic_upgrade` + `embedded` / `absent` / no row          | AMLL → LRCLIB → LrcApi | Any timed online winner |
| `automatic_upgrade` + manual / sidecar / `amll`               | None (no HTTP)         | Nothing                 |
| `user_replace`                                                | AMLL → LRCLIB → LrcApi | Any existing row        |

Probe-fresh upgrade is `NotApplicable`: persist unchanged, no publish. Command still returns the current cache row.

### Command: `import_lyrics_files`

**Input**

```json
{
  "paths": ["/path/to/song.lrc", "/path/to/another.txt"]
}
```

**Output**

```json
{
  "matched": [
    {
      "song_id": "sha256 hash string",
      "lrc_path": "/path/to/song.lrc",
      "song_title": "All for Nothing",
      "song_artist": "Linkin Park"
    }
  ],
  "unmatched": ["/path/to/not_found.txt"]
}
```

**Semantics**

1. 逐个读取 `paths` 中的文件，尝试用文件名（去扩展名）或 LRC 元数据中的 `[ti:]`/`[ar:]` 匹配本地库中的歌曲
2. 匹配成功且缓存写入成功 → 加入 `matched`；匹配失败或缓存写入失败 → 加入 `unmatched`
3. 缓存写入失败（SQLite 错误）时，文件会被加入 `unmatched` 并在 stderr 记录错误（包含 song hash 和文件路径），不会静默丢弃
4. 该命令不发起网络请求，只写本地 SQLite cache
5. `source` 固定为 `manual`（或 `manual_ttml` / `manual_lys`，取决于文件内容格式）

### Command: `set_lyrics_offset`

**Input**

```json
{
  "song_id": "sha256 hash string",
  "ms": 500
}
```

**Semantics**

1. `ms` 为该歌曲的用户手动 timing offset，单位毫秒，可正可负
2. 只有在该歌曲已经存在缓存歌词时，命令才会成功
3. 如果歌曲存在但还没有缓存歌词，命令返回 `CommandError`
4. 该命令只更新 SQLite 中的 `offset_ms`，不会重抓歌词

### Command: `save_manual_lyrics`

**Input**

```json
{
  "song_id": "sha256 hash string",
  "text": "[00:35.66]Look at the stars"
}
```

**Output:** `LyricsPayload` — the parsed lyrics with the detected source.

**Semantics**

1. The command parses the `text` as LRC, TTML, or plain text
2. The backend detects the source type from the content: `manual_ttml` for XML-starting text, `manual_lys` for LRC-starting text, `manual` for plain text
3. The command writes the raw text, detected source, and parsed offset to the SQLite `lyrics` cache
4. The command publishes the updated lyrics to the active remote library if one is connected
5. If the text cannot be parsed as timed lyrics, the backend falls back to plain-text lines
6. If the song does not exist, the command returns `CommandError`

### Command: `set_lyrics_font_step`

**Input**

```json
{
  "step": 1
}
```

**Output**

```json
{
  "stem_mode": "two_stem",
  "model_variant": "htdemucs",
  "language": "en",
  "hide_batch_separate": false,
  "hide_upgrade_all": false,
  "lyrics_font_step": 1,
  "library_sort_mode": "recently_imported"
}
```

**Semantics**

1. `step` 是全局歌词字号档位，允许值固定为 `-2..2`
2. 该命令将字号档位持久化到应用 `config.json`
3. 该命令不会修改 SQLite `lyrics` 表，也不会影响歌词抓取或 timing offset
4. 超出范围时命令返回 `CommandError`

### Shared type: `LyricsPayload`

| Field       | Type                   | Notes                                                           |
| ----------- | ---------------------- | --------------------------------------------------------------- |
| `song_id`   | `String`               | 对应 `songs.hash`                                               |
| `lines`     | `Vec<LyricLine>`       | 已按 `time_ms` 升序排序                                         |
| `source`    | `Option<LyricsSource>` | 无命中时为 `null`                                               |
| `offset_ms` | `i64`                  | 当前已持久化的 timing offset                                    |
| `raw_lrc`   | `String`               | 原始歌词文本；字段名保留历史命名，但内容可能是 LRC、TTML 或 LYS |

### Shared type: `LyricsSource`

| Serialized value | Meaning                                           |
| ---------------- | ------------------------------------------------- |
| `lrc_lib`        | LRCLIB timed LRC                                  |
| `lrc_api`        | LrcApi timed LRC                                  |
| `lrc_api_ttml`   | LrcApi TTML payload                               |
| `amll`           | AMLL native TTML that parsed as Word-timed Lyrics |
| `embedded`       | Audio tag embedded lyrics                         |
| `sidecar`        | Same-name `.lrc` sidecar                          |
| `sidecar_ttml`   | Same-name `.ttml` sidecar                         |
| `sidecar_lys`    | Same-name `.lys` sidecar                          |
| `manual`         | User-saved manual LRC/plain text                  |
| `manual_ttml`    | User-saved manual TTML                            |
| `manual_lys`     | User-saved manual LYS                             |
| `absent`         | Negative cache (all sources miss, 7-day TTL)      |

### Shared type: `LyricLine`

| Field      | Type                     | Notes                                       |
| ---------- | ------------------------ | ------------------------------------------- |
| `time_ms`  | `u64`                    | 行起始时间，单位毫秒                        |
| `text`     | `String`                 | 当前时间戳对应显示的主歌词文本              |
| `words`    | `Option<Vec<WordToken>>` | 主唱逐词 timing；LRC/plain line 可为 `null` |
| `bg_words` | `Option<Vec<WordToken>>` | 背景人声逐词 timing；无背景人声时为 `null`  |
| `section`  | `Option<String>`         | TTML section/song-part，例如 verse/chorus   |
| `roman`    | `Option<String>`         | Supplied Romanization; absent when `null`   |

### Shared type: `WordToken`

| Field     | Type             | Notes                                                          |
| --------- | ---------------- | -------------------------------------------------------------- |
| `time_ms` | `u64`            | 单词开始时间，单位毫秒                                         |
| `end_ms`  | `u64`            | 单词结束时间，单位毫秒                                         |
| `text`    | `String`         | 单词或 syllable 显示文本                                       |
| `roman`   | `Option<String>` | 与该词对齐的罗马音。无对齐发音时为 `null`。缺省字段视为 `null` |

### Shared error type: `CommandError`

歌词命令统一返回结构化错误，字段定义与错误码含义见 [errors.md](./errors.md)。

## Cache semantics

1. SQLite `lyrics` 表字段固定为：
   - `song_hash`
   - `lrc`（历史字段名；内容是原始歌词文本，可为 LRC/TTML/LYS）
   - `source`
   - `offset_ms`
   - `fetched_at`
   - `word_timed_checked_at`（nullable Unix seconds；Word-timed Upgrade probe，7 天 TTL，不出现在 `LyricsPayload`）
2. 当 embedded、sidecar 和全部在线 provider（AMLL、LRCLIB、LrcApi）都确定缺失时，后端会写入一条 `source = absent` 的负缓存行，避免在短期内重复发起网络请求
3. 负缓存行有 7 天 TTL（`NEGATIVE_CACHE_TTL_SECS`）。超过 TTL 后，`fetch_lyrics` / `fetch_lyrics_online` 会跳过缓存重新执行完整查找链，以便发现后续被添加到 AMLL/LRCLIB/LrcAPI 的歌词
4. 网络错误（非 definitive miss）不会写入负缓存
5. `source` 序列化值固定为：
   - `lrc_lib`（IPC；SQLite 仍为历史值 `lrclib`）
   - `lrc_api`
   - `lrc_api_ttml`
   - `amll`（IPC 与 SQLite 均为 `amll`）
   - `embedded`
   - `sidecar`
   - `sidecar_ttml`
   - `sidecar_lys`
   - `manual`
   - `manual_ttml`
   - `manual_lys`
   - `absent`（内部负缓存，7 天 TTL；不出现在 `LyricsPayload.source`）

## Required dependencies

1. `reqwest` 负责 AMLL、LRCLIB 和 LrcApi HTTP 请求
2. `lofty` 负责读取内嵌歌词标签
3. `quick-xml` 负责 TTML 解析
4. `regex` 负责 LYS token 解析
5. `rusqlite` 负责缓存和 offset 持久化
6. `playback-position` 事件由播放契约（`playback.md`）提供，歌词契约本身不新增事件
7. 全局显示偏好由 settings 命令提供；歌词模块当前额外依赖 `AppSettings.lyrics_font_step`
8. `unicode-normalization` 负责 AMLL 标题/艺人匹配的 NFKC 规范化
