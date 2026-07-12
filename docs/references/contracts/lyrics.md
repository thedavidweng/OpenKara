# 歌词契约

覆盖歌词 IPC：LRCLIB client、LrcApi client、LRC/TTML/LYS parser、抓取优先链、SQLite cache，以及 `fetch_lyrics / fetch_lyrics_online / set_lyrics_offset / import_lyrics_files` 命令。

## 接口

1. `fetch_lyrics(song_id: String) -> LyricsPayload`
2. `set_lyrics_offset(song_id: String, ms: i64) -> ()`
3. `set_lyrics_font_step(step: i8) -> AppSettings`
4. 抓取优先顺序固定为 `LRCLIB -> LrcApi -> embedded -> sidecar`
5. sidecar 优先级固定为 `.ttml -> .lys -> .lrc`；每个候选格式必须先能解析出至少一行歌词，格式错误时继续尝试下一种
6. SQLite `lyrics` 表按 `song_hash` 缓存原始歌词文本和 `offset_ms`
7. 对同一首歌重复调用 `fetch_lyrics` 时，优先命中 SQLite cache，不重复发起 HTTP 请求
8. 歌词字号是全局显示偏好，不写入 `lyrics` 表；它走 `AppSettings.lyrics_font_step`
9. 歌词命令失败值统一为 `CommandError`，详见 [errors.md](./errors.md)

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
          "text": "Look"
        }
      ],
      "bg_words": null,
      "section": null
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
   - LRCLIB `GET /api/get`
   - LrcApi `GET /jsonapi`；优先用 `lrc`，没有 synced LRC 时可用 `lrc_ttml`
   - 音频文件内嵌歌词标签
   - 同名 sidecar `.ttml`
   - 同名 sidecar `.lys`
   - 同名 sidecar `.lrc`
4. 一旦抓到歌词，后端会先解析成 `Vec<LyricLine>`，再把原始歌词文本、来源和 `offset_ms = 0` 写入 SQLite
5. 在线 provider 的请求失败或 `jsonapi` 返回 `{"message":"未找到歌词"}` 时，不会中断后续 provider / 本地来源的查找
6. 如果所有来源都 miss，命令仍然成功返回；只是 `lines = []`、`source = null`
7. 如果歌曲不存在、文件读取失败或歌词解析失败，命令返回 `CommandError`

### Command: `fetch_lyrics_online`

**Input**

```json
{
  "song_id": "sha256 hash string"
}
```

**Semantics**

1. 仅尝试在线 timed lyrics provider，不读取 embedded 或 sidecar
2. 在线 provider 顺序固定为 `LRCLIB -> LrcApi`
3. 如果两个 provider 都 miss，命令返回空 payload，而不是写入缓存
4. 如果任一 provider 命中，返回的 `LyricsPayload` 与 `fetch_lyrics` 保持一致，并将结果写入 SQLite cache
5. 如果所有在线 provider 都因为请求或响应错误而无法返回 timed lyrics，命令返回 `CommandError`

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
  "lyrics_font_step": 1
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

| Serialized value | Meaning                                      |
| ---------------- | -------------------------------------------- |
| `lrc_lib`        | LRCLIB timed LRC                             |
| `lrc_api`        | LrcApi timed LRC                             |
| `lrc_api_ttml`   | LrcApi TTML payload                          |
| `embedded`       | Audio tag embedded lyrics                    |
| `sidecar`        | Same-name `.lrc` sidecar                     |
| `sidecar_ttml`   | Same-name `.ttml` sidecar                    |
| `sidecar_lys`    | Same-name `.lys` sidecar                     |
| `manual`         | User-saved manual LRC/plain text             |
| `manual_ttml`    | User-saved manual TTML                       |
| `manual_lys`     | User-saved manual LYS                        |
| `absent`         | Negative cache (all sources miss, 7-day TTL) |

### Shared type: `LyricLine`

| Field      | Type                     | Notes                                       |
| ---------- | ------------------------ | ------------------------------------------- |
| `time_ms`  | `u64`                    | 行起始时间，单位毫秒                        |
| `text`     | `String`                 | 当前时间戳对应显示的主歌词文本              |
| `words`    | `Option<Vec<WordToken>>` | 主唱逐词 timing；LRC/plain line 可为 `null` |
| `bg_words` | `Option<Vec<WordToken>>` | 背景人声逐词 timing；无背景人声时为 `null`  |
| `section`  | `Option<String>`         | TTML section/song-part，例如 verse/chorus   |

### Shared type: `WordToken`

| Field     | Type     | Notes                    |
| --------- | -------- | ------------------------ |
| `time_ms` | `u64`    | 单词开始时间，单位毫秒   |
| `end_ms`  | `u64`    | 单词结束时间，单位毫秒   |
| `text`    | `String` | 单词或 syllable 显示文本 |

### Shared error type: `CommandError`

歌词命令统一返回结构化错误，字段定义与错误码含义见 [errors.md](./errors.md)。

## Cache semantics

1. SQLite `lyrics` 表字段固定为：
   - `song_hash`
   - `lrc`（历史字段名；内容是原始歌词文本，可为 LRC/TTML/LYS）
   - `source`
   - `offset_ms`
   - `fetched_at`
2. 当所有来源（LRCLIB、LrcApi、embedded、sidecar）都 miss 时，后端会写入一条 `source = absent` 的负缓存行，避免在短期内重复发起网络请求
3. 负缓存行有 7 天 TTL（`NEGATIVE_CACHE_TTL_SECS`）。超过 TTL 后，`fetch_lyrics` / `fetch_lyrics_online` 会跳过缓存重新执行完整查找链，以便发现后续被添加到 LRCLIB/LrcAPI 的歌词
4. 网络错误（非 definitive miss）不会写入负缓存
5. `source` 序列化值固定为：
   - `lrc_lib`
   - `lrc_api`
   - `lrc_api_ttml`
   - `embedded`
   - `sidecar`
   - `sidecar_ttml`
   - `sidecar_lys`
   - `manual`
   - `manual_ttml`
   - `manual_lys`
   - `absent`（负缓存，7 天 TTL）

## Required dependencies

1. `reqwest` 负责 LRCLIB 和 LrcApi HTTP 请求
2. `lofty` 负责读取内嵌歌词标签
3. `quick-xml` 负责 TTML 解析
4. `regex` 负责 LYS token 解析
5. `rusqlite` 负责缓存和 offset 持久化
6. `playback-position` 事件由播放契约（`playback.md`）提供，歌词契约本身不新增事件
7. 全局显示偏好由 settings 命令提供；歌词模块当前额外依赖 `AppSettings.lyrics_font_step`

## Verification commands

```bash
cd src-tauri
cargo test --test phase4_lrclib --test phase4_lrcapi --test phase4_parser --test phase4_fetch --test phase4_lyrics_cache --test phase4_commands --test phase5_errors
cargo test
cd ..
pnpm tauri build --debug --no-bundle --ci
```

**Expected evidence**

1. `phase4_lrclib`
2. `phase4_lrcapi`
3. `phase4_parser`
4. `phase4_fetch`
5. `phase4_lyrics_cache`
6. `phase4_commands`
7. `phase5_errors`

以上测试全部通过，并且调试构建成功。
