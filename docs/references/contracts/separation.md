# 分离契约

`separate`、`upgrade_to_four_stem`、`re_separate`、`batch_separate` 的后台编排收口到 `services::separation`（命令层仅为 IPC 适配器），命令名、事件名和状态快照契约保持不变。

## 接口

1. `separate(song_id: String) -> SeparationStatusSnapshot`
2. `upgrade_to_four_stem(song_id: String) -> SeparationStatusSnapshot`
3. `re_separate(song_id: String, stem_mode: StemMode) -> SeparationStatusSnapshot`
4. `get_separation_status(song_id: String) -> SeparationStatusSnapshot`
5. `get_all_separation_statuses() -> Vec<SeparationStatusSnapshot>`
6. `cancel_separation(song_id: String) -> ()`
7. `downgrade_single_to_two_stem(song_id: String) -> SeparationStatusSnapshot`
8. `batch_separate(song_ids: Vec<String>) -> ()`
9. `cancel_batch_separation() -> ()`
10. `separation-progress` 事件 payload 为 `{ song_id: String, percent: u8 }`
11. `separation-complete` 事件 payload 为 `{ song_id: String, status: SeparationStatusSnapshot }`
12. `separation-error` 事件 payload 为 `{ song_id: String, error: CommandError }`
13. `separation-cancelled` 事件 payload 为 `{ song_id: String }`
14. `batch-separation-progress` 事件 payload 为 `BatchSeparationProgress`
15. `batch-separation-complete` 事件 payload 为 `BatchSeparationProgress`
16. `batch-separation-cancelled` 事件 payload 为 `BatchSeparationProgress`
17. stem cache 目录固定为 `<app_cache_dir>/stems/{song_hash}/`
18. `separate(song_id)` 只有在模型 bootstrap 为 `ready` 时才会真正启动后台 worker
19. 分离前会把输入音频归一化为 Demucs 需要的 `44.1 kHz / stereo`
20. 超过单个 Demucs window 的长音频会按固定窗口分段推理并拼回完整 stems

## Inputs / outputs / required dependencies

### Command: `separate`

**Input**

```json
{
  "songId": "sha256 hash string"
}
```

**Output**

```json
{
  "songId": "sha256 hash string",
  "state": "running",
  "percent": 0,
  "cacheHit": false,
  "vocalsPath": null,
  "accompPath": null,
  "error": null
}
```

**Semantics**

1. 如果同一首歌已经在分离中，命令直接返回现有 `running` 状态，不重复启动 worker
2. 命令本身立即返回；实际推理在后台 `spawn_blocking` worker 中执行
3. worker 会按阶段更新进度，并发出 progress / complete / error 事件
4. 如果缓存命中，后台仍会发出一次 `separation-progress`，其 `percent` 为 `100`，然后再发 `separation-complete`
5. 标记为 `instrumental = true` 的歌曲视为官方伴奏，不允许进入 AI 分离
6. Runtime/model 缺失不阻止命令创建后台任务；worker 会按 Runtime -> active model -> separation 的顺序自动补齐前置资源。下载、校验或 outdated 模型失败时，任务通过 terminal error 状态返回；模型侧约束详见 [model-bootstrap.md](./model-bootstrap.md)
7. **ONNX Runtime 前置检查:** 分离命令（`separate`、`upgrade_to_four_stem`、`re_separate`、`batch_separate`）现在会在后台 worker 中确保 ONNX Runtime 已就绪。若 Runtime 缺失或损坏，bootstrap worker 会下载、校验、探测并暂存 Runtime；应用进程会在独立 watchdog 下加载 ORT，再完成首次激活，然后继续模型 bootstrap 与分离。Runtime 状态通过 `get_runtime_bootstrap_status` IPC 和 `runtime-bootstrap-*` 事件查询。Windows 目录可同时存在多个 active runtime（例如 DirectML 版与 CPU-only 版），`resolve_runtime` 会按首选执行提供商挑选对应 artifact；若 DirectML 加载超时，后端会记录 `directml_disabled_by_runtime_timeout` 标记并在下次解析时回落到 CPU-only runtime，`runtime-bootstrap-*` 事件 payload 中会带上 `cpu_fallback_notice` 字段（详见 ADR 0023）。

### Command: `upgrade_to_four_stem`

**Semantics**

1. 命令会强制使用 `four_stem` 目标执行分离
2. 如果当前歌曲已经有完整四轨缓存，命令直接返回 `completed` 状态，不重复启动 worker
3. `instrumental` 歌曲同样不会进入该命令的分离链路
4. 其余后台执行、事件和错误语义与 `separate` 保持一致

### Command: `re_separate`

**Semantics**

1. 命令会先删除已有 stem cache 记录和文件，再重新启动后台分离
2. 启动前会移除内存中的旧状态，使歌曲先回到“重新运行”的干净状态
3. `instrumental` 歌曲不会进入该命令的重新分离链路
4. 目标 stem 模式由参数显式给出，不依赖当前缓存状态

### Command: `get_separation_status`

**Input**

```json
{
  "songId": "sha256 hash string"
}
```

**Semantics**

1. 如果该歌曲还没有任何分离记录，返回 `idle` 状态
2. `completed` 状态会带上 `vocalsPath` 和 `accompPath`
3. `failed` 状态会带上结构化错误 `CommandError`

### Shared type: `SeparationStatusSnapshot`

| Field        | Type                                             | Notes                          |
| ------------ | ------------------------------------------------ | ------------------------------ |
| `songId`     | `String`                                         | 对应 `songs.hash`              |
| `state`      | `"idle" \| "running" \| "completed" \| "failed"` | 状态字段固定为 snake_case enum |
| `percent`    | `u8`                                             | `0..100`                       |
| `cacheHit`   | `bool`                                           | 仅 `completed` 时可能为 `true` |
| `vocalsPath` | `Option<String>`                                 | `completed` 时存在             |
| `accompPath` | `Option<String>`                                 | `completed` 时存在             |
| `error`      | `Option<CommandError>`                           | `failed` 时存在                |

### Events

#### `separation-progress`

```json
{
  "songId": "sha256 hash string",
  "percent": 70
}
```

#### `separation-complete`

```json
{
  "song_id": "sha256 hash string",
  "status": {
    "song_id": "sha256 hash string",
    "state": "completed",
    "percent": 100,
    "cache_hit": false,
    "vocals_path": "stems/song/vocals.ogg",
    "accomp_path": "stems/song/accompaniment.ogg",
    "drums_path": null,
    "bass_path": null,
    "other_path": null,
    "model_variant": "two_stem",
    "error": null
  }
}
```

#### `separation-error`

```json
{
  "songId": "sha256 hash string",
  "error": {
    "code": "separation_failed",
    "message": "failed to separate stems for song song-a",
    "retryable": true,
    "fallback": "retry"
  }
}
```

### Command: `get_all_separation_statuses`

**Input**: none

**Output:** `Vec<SeparationStatusSnapshot>`

**Semantics**

1. The frontend calls this command once at startup to hydrate the separation status store
2. The command reads all cached stem entries from the SQLite `stems` table
3. Only entries whose stem files still exist on disk are returned as `completed`
4. The command also populates the in-memory separation status map so subsequent `get_separation_status` calls return the correct state
5. Songs without any cached stems are not included in the returned vector

### Command: `cancel_separation`

**Input**

```json
{
  "songId": "sha256 hash string"
}
```

**Output:** `()`

**Semantics**

1. Sets the cancellation flag for the given song if a separation job is currently running
2. A cancelled run never surfaces a `separation-error` event or an error toast
3. If the song is not currently separating, the command is a benign no-op success
4. The backend emits `separation-cancelled` after the worker observes the flag and stops

### Command: `downgrade_single_to_two_stem`

**Input**

```json
{
  "songId": "sha256 hash string"
}
```

**Output:** `SeparationStatusSnapshot`

**Semantics**

1. Removes the individual drum/bass/other stem files for the given song and updates the database entry to two-stem
2. The command emits a `separation-complete` event with the updated `completed` status
3. `cacheHit` is `false` because a downgrade is an explicit user action, not a cache-served separation
4. The command publishes the updated song to the active remote library if one is connected
5. If the song does not have individual stems, the command returns the current `completed` status

### Command: `batch_separate`

**Input**

```json
{
  "songIds": ["sha256 hash string"]
}
```

Pass an empty `songIds` vector to separate all separable songs in the library.

**Output:** `()`

**Semantics**

1. If a batch separation is already running, the command returns `CommandError`
2. The command plans the batch: songs with valid cached stems for the active stem mode are skipped and counted in `skipped`
3. Songs marked `instrumental = true` or Media+G are excluded from the plan
4. Jobs run sequentially because ONNX Runtime is memory-heavy
5. The command returns immediately; the batch loop runs in the background
6. The backend emits `batch-separation-progress` events during the batch and a terminal `batch-separation-complete` event
7. Each song in the batch also emits the standard `separation-progress`, `separation-complete`, and `separation-error` events
8. Runtime and model bootstrap happen once before the batch loop starts; if bootstrap fails, the batch emits a terminal `batch-separation-complete` event with all candidates as `failed`

### Command: `cancel_batch_separation`

**Input**: none

**Output:** `()`

**Semantics**

1. If no batch separation is running, the command returns `CommandError`
2. The command sets the batch cancel flag and flags the in-flight song so the batch stops mid-song
3. The backend emits `batch-separation-cancelled` after the current song stops
4. Songs that were not yet processed remain unseparated

### Event: `separation-cancelled`

```json
{
  "songId": "sha256 hash string"
}
```

**Semantics**

1. Emitted when a cancellation flag was set and the worker observed it before completing
2. A cancelled run never emits `separation-error`; the frontend resets the song status to `idle`
3. A run that completed successfully before the cancellation flag was observed does not emit this event

### Event: `batch-separation-progress`

```json
{
  "total": 15,
  "completed": 3,
  "skipped": 5,
  "failed": 0,
  "currentSongId": "sha256 hash string",
  "currentPercent": 42
}
```

**Semantics**

1. `total` is the number of candidate songs to separate (excludes skipped songs)
2. `skipped` is the number of songs that already had valid cached stems for the active stem mode
3. `currentSongId` is `null` between songs or when the batch has not started processing
4. `currentPercent` is the per-song progress of the current song (0–100)
5. The backend emits this event at the start of the batch, before each song, and on each per-song progress update

### Event: `batch-separation-complete`

Payload is the same `BatchSeparationProgress` shape as `batch-separation-progress`.

**Semantics**

1. Emitted when the batch loop finishes all candidate songs
2. `completed` + `failed` + `skipped` = `total` + `skipped`
3. `currentSongId` is `null` and `currentPercent` is `0`
4. If the prerequisite bootstrap failed, the backend emits this event with all candidates as `failed`

### Event: `batch-separation-cancelled`

Payload is the same `BatchSeparationProgress` shape as `batch-separation-progress`.

**Semantics**

1. Emitted when the user called `cancel_batch_separation` and the batch loop observed the cancel flag
2. `completed` and `failed` reflect the state at the time of cancellation
3. Songs that were not yet processed remain unseparated

### Shared type: `BatchSeparationProgress`

| Field             | Type             | Notes                                             |
| ----------------- | ---------------- | ------------------------------------------------- |
| `total`           | `usize`          | Candidate songs to separate (excludes skipped)    |
| `completed`       | `usize`          | Songs that finished successfully                  |
| `skipped`         | `usize`          | Songs that already had valid cached stems         |
| `failed`          | `usize`          | Songs that failed separation                      |
| `current_song_id` | `Option<String>` | The song being processed, or `null` between songs |
| `current_percent` | `u8`             | Per-song progress of the current song (0–100)     |

### Shared error type: `CommandError`

分离失败状态和 `separation-error` 事件统一复用结构化错误，字段定义与错误码含义见 [errors.md](./errors.md)。

## Cache semantics

1. 完整 stem 输出会写进 `<app_cache_dir>/stems/{song_hash}/`
2. 目录内至少有：
   - `vocals.ogg`
   - `accompaniment.ogg`
3. SQLite `stems` 表记录：
   - `song_hash`
   - `vocals_path`
   - `accomp_path`
   - `separated_at`
4. 如果数据库记录存在但文件丢失，后端会重新生成并覆盖目录
5. 生成的 OGG stem 会复制原曲 metadata，并把 `title` 改写为对应 stem 后缀
6. 命令层现在通过共享分离 helper 统一管理 running 状态复用、进度事件和最终状态写回，避免三个入口出现行为漂移
7. **Streaming OGG writers:** stem 输出通过 streaming writer 增量写入临时文件，完成后原子重命名为最终文件。崩溃或取消不会留下部分缓存文件。
8. **Bounded output working set:** 分离输出使用 OLA ring buffer 和可复用 workspace；除一份完整 normalized 输入 PCM 外，额外内存仅取决于推理窗口大小。

## Required dependencies

1. `symphonia` 负责解码输入音频
2. `ort` 负责 Demucs ONNX 推理
3. `rubato` 负责把非 `44.1 kHz` 输入重采样到 Demucs 目标采样率
4. `vorbis_rs` + backend audio encode helper 负责 stem / accompaniment OGG 写盘
5. `tauri::async_runtime::spawn_blocking` 负责后台执行推理任务

## Limits & expectations

| Dimension              | Measured value / policy                                                                                                                                                                            |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| File size              | No hard cap. The input is decoded and normalized once, then processed in fixed-size chunks. One normalized full-song PCM buffer remains by design.                                                 |
| Duration               | No hard cap. Long audio is processed through fixed Demucs windows and streaming OLA output. Additional output/OLA/encoder working memory stays fixed as duration grows.                            |
| Peak memory            | One normalized full-song input buffer plus fixed-size model/session, OLA, planar input, window, accompaniment scratch, and encoder staging buffers. No full-song stem output buffers are retained. |
| Disk (stems)           | 2-stem: ~1 MB per minute of audio (OGG ~128 kbps). 4-stem: ~2–2.5× 2-stem due to four track files.                                                                                                 |
| Cancellation           | Mid-separation cancellation drops streaming-writer temp files; no partial cache entry is published. The next attempt restarts from chunk 0.                                                        |
| Checkpoint             | No partial-encoding resume. Vorbis encoder and OLA state are not serialized. An interrupted run is cleaned up and restarted from chunk 0.                                                          |
| Concurrent runs        | One separation per process (singleton worker). A second `separate()` call for a different song returns `running` for the first job; the second is queued.                                          |
| Instrumental exemption | Songs with `instrumental = true` never enter the AI separation path and return `completed` immediately with no-op.                                                                                 |

**Reference measurement** (for comparison when evaluating future regressions):

- Hardware: Apple M2, 16 GB RAM, macOS 15.4
- File: 44.1 kHz / 16-bit / stereo WAV, 4:32 duration, 45 MB
- Architecture: one normalized full-song input buffer plus fixed-size streaming OLA rings and atomic streaming OGG writers
- No full-song vocals/drums/bass/other/accompaniment output buffers are retained
- Output: 2-stem produces vocals.ogg + accompaniment.ogg; 4-stem produces vocals/drums/bass/other.ogg
