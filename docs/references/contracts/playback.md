# 播放契约

播放命令层收口为 thin Tauri command，具体编排在 backend playback service / CDG helper，对外 IPC 契约保持不变。

## 接口

1. `play(song_id: String) -> PlaybackStateSnapshot`
2. `resume() -> PlaybackStateSnapshot`
3. `pause() -> PlaybackStateSnapshot`
4. `seek(ms: u64) -> PlaybackStateSnapshot`
5. `set_volume(level: f32) -> PlaybackStateSnapshot`
6. `set_stem_volume(stem: StemName, level: f32) -> PlaybackStateSnapshot`
7. `load_stems() -> PlaybackStateSnapshot`
8. `get_playback_state() -> PlaybackStateSnapshot`
9. `playback-position` 事件 payload 为 `{ ms: u64, transport_generation: u64, snapshot: PlaybackStateSnapshot }`

## Inputs / outputs / required dependencies

### Command: `play`

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
  "isPlaying": true,
  "positionMs": 0,
  "durationMs": 1000,
  "volume": 1.0,
  "stemVolumes": {
    "vocals": 1.0,
    "drums": 1.0,
    "bass": 1.0,
    "other": 1.0
  },
  "hasStems": false,
  "stemMode": null
}
```

**Semantics**

1. `song_id` 对应 `songs.hash`
2. 命令会立即返回 `state: "loading"` 的快照，并在后台线程完成文件读取与 PCM 解码
3. 解码完成后 backend 通过 `playback-position` 事件推送 `state: "playing"` 快照；前端无需再调用 `get_playback_state`
4. 首次真正开始输出时会懒启动 `cpal` 输出线程
5. 如果找不到歌曲，命令返回 `CommandError`；解码/输出失败发生在后台线程时，backend 会清除 loading 并通过 `playback-error` 事件通知前端
6. latest-request-wins：`request_id` 较旧的 decode 结果不会覆盖较新的播放请求

### Command: `pause`

**Output:** `PlaybackStateSnapshot`

**Semantics**

1. 暂停后保留当前位置
2. `isPlaying` 变为 `false`
3. 当前实现不清空已加载轨道

### Command: `resume`

**Output:** `PlaybackStateSnapshot`

**Semantics**

1. 没有已加载轨道时返回 `CommandError`
2. 恢复后从当前暂停位置继续推进
3. 若输出线程尚未启动，命令会和 `play` 一样保证输出线程已就绪

### Command: `seek`

**Input**

```json
{
  "ms": 900
}
```

**Semantics**

1. 会 clamp 到 `0..durationMs`
2. 若当前正在播放，seek 后继续播放
3. 命令完成后会立刻触发一次位置事件

### Command: `set_volume`

**Input**

```json
{
  "level": 0.35
}
```

**Semantics**

1. 取值会 clamp 到 `0.0..1.0`
2. 默认初始音量为 `1.0`
3. 音量状态独立于当前是否有已加载轨道

### Command: `set_stem_volume`

**Input**

```json
{
  "stem": "vocals",
  "level": 0.35
}
```

**Semantics**

1. 取值会 clamp 到 `0.0..1.0`
2. 目标 stem 固定为 `vocals | drums | bass | other`
3. 未加载 stems 时调用仍返回当前快照；不会隐式触发 stem 解码

### Command: `load_stems`

**Output:** `PlaybackStateSnapshot`

**Semantics**

1. 当前歌曲已挂载 stems 时，直接返回现有快照
2. 当前歌曲没有缓存 stems 时，命令返回 `CommandError`
3. stem 解码遵守 stale decode 忽略规则：如果解码完成时当前歌曲已切换，不会把 stems 附着到新歌曲

### Shared type: `PlaybackStateSnapshot`

| Field                  | Type                                              | Notes                                                     |
| ---------------------- | ------------------------------------------------- | --------------------------------------------------------- |
| `song_id`              | `Option<String>`                                  | 当前未加载轨道时为 `null`                                 |
| `transport_generation` | `u64`                                             | 单调 transport 代号；新歌加载、resume、pause、seek 时递增 |
| `state`                | `"idle" \| "loading" \| "playing" \| "buffering"` | 后端 transport 生命周期；暂停由 `is_playing=false` 表示   |
| `is_playing`           | `bool`                                            | 当前是否处于播放推进状态                                  |
| `position_ms`          | `u64`                                             | 当前播放位置（由 `render_frame` 推导，非墙钟）            |
| `duration_ms`          | `Option<u64>`                                     | 未加载轨道时为 `null`                                     |
| `buffered_ms`          | `u64`                                             | 已缓冲的最大安全播放位置（ms）；整轨模式 = `duration_ms`  |
| `volume`               | `f32`                                             | `0.0..1.0`                                                |
| `stem_volumes`         | `{ vocals, drums, bass, other }`                  | 各 stem 音量                                              |
| `has_stems`            | `bool`                                            | 当前是否已挂载 stems                                      |
| `stem_mode`            | `"two_stem" \| "four_stem" \| null`               | 当前 stem 模式                                            |

**Transport state 语义：**

- `idle`：无轨道加载。
- `loading`：首次取数/解码尚未出声。
- `playing`：正常播放（`isPlaying` 区分播放/暂停）。
- `buffering`：已开始播放但缓冲欠载，暂停等待数据（P1+ 流式模式触发）。

**状态转移：**

```
idle → loading（play 命令）
loading → playing（解码完成、出声）
playing ↔ buffering（流式缓冲欠载/恢复，P1+）
playing → idle（clear_track）
playing ↔ playing（pause/resume，通过 isPlaying 区分）
```

### Event: `playback-position`

**Payload**

```json
{
  "ms": 1234,
  "transport_generation": 4,
  "snapshot": {
    "song_id": "abc123",
    "transport_generation": 4,
    "state": "playing",
    "is_playing": true,
    "position_ms": 1234,
    "duration_ms": 180000,
    "buffered_ms": 180000,
    "volume": 1.0,
    "stem_volumes": {
      "vocals": 1.0,
      "drums": 1.0,
      "bass": 1.0,
      "other": 1.0
    },
    "has_stems": false,
    "stem_mode": null
  }
}
```

**Semantics**

1. 事件名固定为 `playback-position`
2. 仅在 snapshot 有 `song_id` 时发出，包括远程音频仍处于 `state="loading"` 的阶段
3. 后端线程约每 `33ms` 检查一次位置，并在位置变化时发出事件
4. `play`、`pause`、`seek`、`resume` 命令执行后也会立即补发一次最新位置
5. `snapshot` 是前端播放状态的权威来源；远端加载从 `loading` 切到 `playing` 时，不需要前端再反查 `get_playback_state`
6. 前端必须丢弃 `transport_generation` 小于当前快照的事件或命令响应；事件顶层 `transport_generation` 必须与 `snapshot.transport_generation` 一致
7. `playback-ended` 是额外内部事件，用于前端队列自动推进；不替代 `playback-position`

### Event: `playback-error`

**Payload**

```json
{
  "song_id": "sha256 hash string",
  "error": {
    "code": "audio_decode_failed",
    "message": "failed to decode audio: ...",
    "retryable": false,
    "fallback": "reimport_song"
  }
}
```

**Semantics**

1. 当 `play` 已返回 `loading` 快照，但后台 decode/换轨失败且该请求仍为 latest 时发出
2. 发出前先通过 `playback-position` 推送 idle snapshot（清除 loading 状态）
3. 前端应调用 `notifyError` 并根据 `error.retryable` 提供重试（通常重试 `play(song_id)`）

### Shared error type: `CommandError`

播放命令统一返回结构化错误，字段定义与错误码含义见 [errors.md](./errors.md)。

### Required dependencies

1. `symphonia` 负责解码支持格式
2. `cpal` 负责设备输出
3. `PlaybackController` 负责状态推进与位置计算
4. backend playback service 负责 latest-request-wins、output thread 启动和 stale decode 忽略
5. backend CDG helper 负责 sidecar / explicit path / Media+G ZIP 的 CDG 状态加载与 backward seek reset
6. `stems` cache 为 `load_stems` 提供已缓存路径

## Verification commands

```bash
cd src-tauri
cargo test
cd ..
pnpm tauri build --debug --no-bundle --ci
```

**Expected evidence**

1. `phase2_decode`
2. `phase2_playback`
3. `phase2_output`

以上测试全部通过，并且调试构建成功。
