# Phase 2 播放契约

**Goal:** 固定 `Phase 2` 代码侧已实现的播放命令、状态快照与位置事件语义，保证后续 UI 和 `cpal` 细化工作都基于同一套契约继续推进。

**Current starting point:** 本契约对应分支 `codex/phase0-m0` 上 `feat: add audio decode pipeline`、`feat: add playback state machine and events`、`feat: add cpal playback output` 之后的状态。

## Owner

- 代码 Agent：播放状态机、解码、设备输出、位置事件
- UI Agent：消费命令返回值和位置事件，不单方面修改命令名、字段名或事件名

## Phase-by-phase task breakdown

### 已冻结能力

1. `play(song_id: String) -> PlaybackStateSnapshot`
2. `pause() -> PlaybackStateSnapshot`
3. `seek(ms: u64) -> PlaybackStateSnapshot`
4. `set_volume(level: f32) -> PlaybackStateSnapshot`
5. `get_playback_state() -> PlaybackStateSnapshot`
6. `playback-position` 事件 payload 为 `{ ms: u64 }`

### 后续 Phase 依赖

1. UI Agent 的 `Player` 组件依赖本契约驱动 seek bar、play/pause、volume 状态
2. `Phase 4` 歌词高亮将依赖 `playback-position`
3. `Phase 5` 的延迟与 jitter 验证会以本快照和事件为基线

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
  "volume": 1.0
}
```

**Semantics**

1. `song_id` 对应 `songs.hash`
2. 命令会从 SQLite 读取歌曲路径，并实时解码为 `f32` PCM
3. 首次播放时会懒启动 `cpal` 输出线程
4. 如果找不到歌曲或无法解码，命令返回错误字符串

### Command: `pause`

**Output:** `PlaybackStateSnapshot`

**Semantics**

1. 暂停后保留当前位置
2. `isPlaying` 变为 `false`
3. 当前实现不清空已加载轨道

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

### Shared type: `PlaybackStateSnapshot`

| Field        | Type             | Notes                     |
| ------------ | ---------------- | ------------------------- |
| `songId`     | `Option<String>` | 当前未加载轨道时为 `null` |
| `isPlaying`  | `bool`           | 当前是否处于播放推进状态  |
| `positionMs` | `u64`            | 当前播放位置              |
| `durationMs` | `Option<u64>`    | 未加载轨道时为 `null`     |
| `volume`     | `f32`            | `0.0..1.0`                |

### Event: `playback-position`

**Payload**

```json
{
  "ms": 1234
}
```

**Semantics**

1. 事件名固定为 `playback-position`
2. 仅在存在已加载轨道时发出
3. 后端线程约每 `16ms` 检查一次位置，并在位置变化时发出事件
4. `play`、`pause`、`seek` 命令执行后也会立即补发一次最新位置

### Required dependencies

1. `symphonia` 负责解码支持格式
2. `cpal` 负责设备输出
3. `PlaybackController` 负责状态推进与位置计算
4. `render_output_buffer` 负责把当前播放状态映射到输出 buffer

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

## Pause-and-resume instructions

1. 接手前先读本文件，再读 [../plans/2026-03-13-code-agent-plan.md](../plans/2026-03-13-code-agent-plan.md)
2. 先跑验证命令，确认播放层没有被后续修改打破
3. 如果后续调整命令名、字段名、事件名或节流语义：
   - 先更新本契约
   - 再改 Rust 实现
   - 最后通知 UI Agent
4. 如果进入真实设备调试阶段，请把：
   - 使用了哪些输入音频
   - 输出设备环境
   - 是否出现卡顿 / seek 偏移 / 静音

写进交接说明。
