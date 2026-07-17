# 播放契约

播放命令层收口为 thin Tauri command，具体编排在 backend playback service / CDG helper，对外 IPC 契约保持不变。

控制面变更（pause / resume / seek / set_volume / set_stem_volume / set_eq_enabled / set_eq_gains / load_stems / install_track / fail_load / prepare_next / cancel_prepared_next）由 `PlaybackCoordinator` 独立线程串行处理；后台 decode/fetch 线程只产出 `ReadyTrack` 并发送命令，不直接修改 `PlaybackController`。

## 接口

1. `play(song_id: String) -> PlaybackStateSnapshot`
2. `resume() -> PlaybackStateSnapshot`
3. `pause() -> PlaybackStateSnapshot`
4. `seek(ms: u64) -> PlaybackStateSnapshot`
5. `set_volume(level: f32) -> PlaybackStateSnapshot`
6. `set_stem_volume(stem: StemName, level: f32) -> PlaybackStateSnapshot`
7. `load_stems() -> PlaybackStateSnapshot`
8. `get_playback_state() -> PlaybackStateSnapshot`
9. `get_audio_peaks() -> AudioPeakSnapshot` — 只读命令，拷贝 lock-free peak ring 快照（不持 playback mutex）
10. `set_preload_candidate(song_id: Option<String>) -> ()` — #88 无缝播放预加载命令（见下）
11. `playback-position` 事件 payload 为 `{ ms: u64, transport_generation: u64, snapshot: PlaybackStateSnapshot }`
12. `track-transitioned` 事件 payload 为 `{ transition_serial: u64, from_song_id: String, to_song_id: String }` — #88 无缝换轨通知（见下）

### Peak envelope 可视化（#87）

`get_audio_peaks` 返回 `AudioPeakSnapshot { writeIndex: u64, peaks: [[f32; 2]; N] }`。

- CPAL 输出回调每 512 帧发布一对 stereo peak（取窗口内 |sample| 最大值，sanitize 后 clamp 到 `[0, 1]`）。
- Ring buffer 容量固定 256 对（约 3.0 s @ 44.1 kHz），单写多读，全原子操作。
- 命令只读 ring，不持 `PlaybackController` mutex，不影响播放实时性。
- 前端以 30 Hz 轮询，DPR-aware canvas 渲染，`writeIndex` 不变时跳过重绘。

### EQ 命令（通过 settings 命令面下发）

13. `set_eq_enabled(enabled: bool) -> AppSettings`
14. `set_eq_gains(gains_db: [f32; 5]) -> AppSettings`

- `set_eq_enabled(enabled: bool) -> AppSettings` — 启用/禁用五段均衡器
- `set_eq_gains(gains_db: [f32; 5]) -> AppSettings` — 设置五个频段增益（dB），范围 [-12, 12]，拒绝越界值而非截断

设置命令执行顺序：验证输入 → 读取旧值 → 发送 coordinator 更新并等待确认 → 持久化 config → 持久化失败则回滚 coordinator → 返回 `settings_from_config`。

`PlaybackController` 在 `setup_app()` 中从持久化 config 初始化 EQ 状态（`eq_enabled` / `eq_gains_db`），在 coordinator/output 线程启动前生效。

### 渲染管线

```text
existing source/stem mix + master/stem gains
→ EQ dry/wet processor + auto preamp
→ soft limiter
→ existing play/pause/seek fade
→ peak envelope accumulator (512-frame window → lock-free ring)
→ output/AirPlay forwarding
```

`EqProcessor` 由 CPAL output 闭包拥有（与 `ResamplerCache` 并列），不存储在 playback mutex 后面。回调在已持有 controller 锁时比较 `eq_revision`，通过 `apply_config` 将配置同步到本地 processor。增益、前置增益和 dry/wet 过渡按渲染帧数平滑推进（50 ms EQ / 20 ms bypass），零长度/buffering 回调不推进状态。

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

### Command: `set_preload_candidate` (#88)

**Input**

```json
{
  "songId": "sha256 hash string"
}
```

传入 `null` 取消当前预加载。

**Output:** `()`（空）

**Semantics**

1. 前端在队列头部或当前歌曲变化时调用此命令，将下一首歌曲预解码为无缝播放候选
2. 命令立即返回；解码在后台线程完成，完成后向 coordinator 发送 `PrepareNext` 命令
3. 只有本地、非流式、非 Media+G 的歌曲符合无缝预加载条件；远程歌曲和 Media+G 容器静默跳过，前端回退到 `play()` 路径
4. 预加载线程使用独立的 `preload_shutdown` 标志，与 `play()` 的 `background_shutdown` 隔离——取消预加载不会中断正在进行的 `play()` 后台解码
5. 传入 `null` 或新候选时，先发送 `CancelPreparedNext`（携带新的 `expected_generation`）清除已安装的 prepared track 并更新 coordinator 的期望预加载代，再启动新的预加载
6. coordinator 在安装前验证 output format generation 和 preload request generation：如果输出设备重启/格式变化，或者 prepared payload 来自已被取消的旧预加载线程（竞态：旧线程通过 shutdown 检查后在 cancel 之后才发送），prepared payload 被丢弃

### Shared type: `PlaybackStateSnapshot`

| Field                  | Type                                              | Notes                                                                              |
| ---------------------- | ------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `song_id`              | `Option<String>`                                  | 当前未加载轨道时为 `null`                                                          |
| `transport_generation` | `u64`                                             | 单调 transport 代号；新歌加载、resume、pause、seek、无缝换轨（gapless swap）时递增 |
| `state`                | `"idle" \| "loading" \| "playing" \| "buffering"` | 后端 transport 生命周期；暂停由 `is_playing=false` 表示                            |
| `is_playing`           | `bool`                                            | 当前是否处于播放推进状态                                                           |
| `position_ms`          | `u64`                                             | 当前播放位置（由 `render_frame` 推导，非墙钟）                                     |
| `duration_ms`          | `Option<u64>`                                     | 未加载轨道时为 `null`                                                              |
| `buffered_ms`          | `u64`                                             | 已缓冲的最大安全播放位置（ms）；整轨模式 = `duration_ms`                           |
| `volume`               | `f32`                                             | `0.0..1.0`                                                                         |
| `stem_volumes`         | `{ vocals, drums, bass, other }`                  | 各 stem 音量                                                                       |
| `has_stems`            | `bool`                                            | 当前是否已挂载 stems                                                               |
| `stem_mode`            | `"two_stem" \| "four_stem" \| null`               | 当前 stem 模式                                                                     |

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
2. 输出设备启动失败时也发出此事件（`InstallReady` 在 coordinator 中完成轨道安装后尝试启动输出线程，若失败则通过 `clear_track_if_matching` 清除已安装的轨道并发出 `playback-error`）
3. 发出前先通过 `playback-position` 推送 idle snapshot（清除已安装的轨道）
4. 前端应调用 `notifyError` 并根据 `error.retryable` 提供重试（通常重试 `play(song_id)`）

### Event: `track-transitioned` (#88)

**Payload**

```json
{
  "transition_serial": 1,
  "from_song_id": "sha256 hash string",
  "to_song_id": "sha256 hash string"
}
```

**Semantics**

1. 当音频回调检测到当前轨道到达 EOF 且有 prepared track 可用时，执行无缝换轨并 stamp 一个 `CompletedTransition`
2. position emitter 线程在下一次轮询时 drain 该 transition，发出 `track-transitioned` 事件，然后发出携带新 `song_id` 的 `playback-position` 事件
3. 事件在 `playback-position` 之前发出，所以前端 clock 可能仍持有 `from_song_id`——前端 reconciliation 必须接受 `from_song_id` 或 `to_song_id` 作为当前歌曲
4. 前端收到事件后：将 `from_song_id` 推入播放历史，从队列中移除 `from_song_id` 和 `to_song_id`
5. `transition_serial` 单调递增，可用于去重或调试
6. 如果前端 clock 持有的 `song_id` 既不是 `from_song_id` 也不是 `to_song_id`（用户手动切换了歌曲），则忽略该事件
7. 无缝换轨时 `transport_generation` 递增，使前端 generation 过滤器丢弃旧歌的延迟 `playback-position` 事件（#103）

### Shared error type: `CommandError`

播放命令统一返回结构化错误，字段定义与错误码含义见 [errors.md](./errors.md)。

### Required dependencies

1. `symphonia` 负责解码支持格式
2. `cpal` 负责设备输出
3. `PlaybackController` 负责状态推进与位置计算
4. `PlaybackCoordinator` 负责串行处理所有控制面命令（pause / resume / seek / set_volume / set_stem_volume / set_eq_enabled / set_eq_gains / install_track / fail_load / attach_stems / prepare_next / cancel_prepared_next），保证 FIFO 顺序与 latest-request-wins
5. backend playback service 负责 latest-request-wins、output thread 启动和 stale decode 忽略
6. backend CDG helper 负责 sidecar / explicit path / Media+G ZIP 的 CDG 状态加载与 backward seek reset
7. `stems` cache 为 `load_stems` 提供已缓存路径
8. `biquad` crate 提供五段 peaking EQ biquad 滤波器系数
9. `EqProcessor` 在实时输出回调中执行 EQ dry/wet 混合 + auto preamp + soft limiter

### Render order

实时输出回调的渲染顺序：

```text
existing source/stem mix + master/stem gains
→ EQ dry/wet processor + auto preamp
→ soft limiter
→ existing play/pause/seek fade
→ peak envelope accumulator (512-frame window → lock-free ring)
→ output/AirPlay forwarding
```

EQ 平滑（gain、preamp、bypass dry/wet）仅在已渲染样本上推进，trailing padding 不推进滤波器状态。Peak 累加在 fade 之后、输出转发之前执行，只统计已渲染样本。

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
