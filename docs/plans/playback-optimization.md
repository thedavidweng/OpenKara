# OpenKara 播放优化计划：流式解码 + 多轨低延迟 + 低带宽韧性

> Status: **implemented**（P0–P5 全部完成，已合并 main）。
>
> 目标：把 OpenKara 的播放体验拉齐到原生本地音乐播放器——多轨（伴奏/人声）启播延迟低、内存占用可控；对网盘/远程库在低带宽下边缓冲边播、不预先整文件下载，并具备欠载韧性。
>
> 本文档是 [`native-feel-optimization.md`](./native-feel-optimization.md) Phase 6 中"整轨预解码 tradeoff"的承接 epic。

---

## 1. 目标与非目标

### 目标

1. **低启播延迟**：双击到出声的时间不随歌曲时长/音质线性增长；hi-res、四轨模式也应在固定的小缓冲后即出声。
2. **内存可控**：播放占用不随曲长线性膨胀。当前四轨 hi-res 会同时驻留 4 份整轨 PCM，必须改为有界缓冲。
3. **多轨样本级同步**：流式化之后，2/4 条 stem 仍严格对齐，切换人声/伴奏不产生相位或时间错位。
4. **远程低带宽韧性**：远程库（Dropbox / Google Drive / 未来 WebDAV 等）边下边播，不整文件预下载；带宽不足时进入显式 `buffering` 态而非卡死或时间漂移。
5. **可恢复**：网络抖动、Range 不支持、URL 过期等情况下有退避重试与回退路径。

### 非目标

- **真正的 ABR（自适应码率）**：OpenKara 远程库是用户自己的**单一画质**原始文件，没有服务端多档转码与 manifest，因此 DASH/HLS 那套 ABR 不适用。低带宽策略走"缓冲 + prefetch + 可选低码率代理"，不做多 rendition 切换。
- **浏览器侧播放**：OpenKara 在 Rust 后端用 `symphonia` 解码、`cpal` 输出，不经过 WebView `<audio>`，因此 Shaka Player / MSE 一律不引入。
- **重写分离（separation）链路**：本计划只动播放/解码/取数路径，不动 Demucs 分离与 stems 产物格式。

---

## 2. 当前架构与瓶颈（基于代码）

### 2.1 解码：整轨一次性进内存

`decode::decode_file` 把整首歌逐 packet 解码后**全部收集进 `Vec<f32>`** 才返回：

```169:213:src-tauri/src/audio/decode.rs
    let mut samples = Vec::new();

    loop {
        let packet = match format.next_packet() {
            ...
        };
        ...
        extend_interleaved_samples(&mut samples, decoded);
    }
    ...
    Ok(DecodedAudio { sample_rate, channels, duration_ms, samples })
```

- 立体声 44.1kHz ≈ 10MB/分钟；hi-res（24bit/192kHz、f32 展开后）更大。
- **四轨模式**：`load_remote_stems_playback_source` / `decode_stem_entry` 会解码 **4 个完整文件**进内存（`StemSet { vocals, drums, bass, other }`），是单轨内存与解码耗时的约 4 倍。

### 2.2 播放控制器：持有整轨 PCM

`PlaybackController.current_track` 持有 `original_audio: DecodedAudio` + 可选 `stems: LoadedStems`，全是整轨缓冲（`src-tauri/src/audio/playback.rs`）。cpal 输出回调 `render_into` 从 `track.render_frame` 起，对每个 stem `mix_stem_resampled` 逐样本叠加增益，设备采样率不同则线性重采样（`src-tauri/src/audio/output.rs:80-240`）。

### 2.3 时钟模型：墙钟，与渲染帧解耦

`position_ms` 由 `started_at_ms + base_position_ms` 推出（墙钟），**不**由 `render_frame` 推导：

```342:349:src-tauri/src/audio/playback.rs
    fn position_ms(&self, now_ms: u64) -> u64 {
        let elapsed_ms = self
            .started_at_ms
            .map(|started_at_ms| now_ms.saturating_sub(started_at_ms))
            .unwrap_or(0);
        (self.base_position_ms + elapsed_ms).min(self.duration_ms())
    }
```

> 隐患：一旦引入流式缓冲，缓冲欠载（underrun）时音频停了但墙钟仍在走 → 进度条跑到音频前面。流式化前必须先解决时钟权威性（§4.4）。

### 2.4 cpal 回调中使用 Mutex

当前 `build_output_stream` 回调通过 `playback.lock()` 取得 `PlaybackController`——在实时音频线程中使用 Mutex 锁。虽然在整轨预加载模式下临界区很短不易出问题，但流式化后必须移除。

### 2.5 远程：整文件下载后才解码

`load_playback_source` 对远程歌曲先 `ensure_remote_song_files_cached`（整文件落地本地缓存），再 `decode_file`；stems 走 `ensure_remote_stem_files_cached` 把 2–4 个 stem 文件**全部**下载完。

低带宽后果：启播 = 下完整文件的时间（四轨更甚），中途断网无渐进缓冲，已下字节不可用于"边下边播"。

---

## 3. 技术选型（已定，基于开源项目证据）

### 3.1 无锁环形缓冲：`ringbuf` v0.4

| 维度         | 决策                                                                                                                     |
| ------------ | ------------------------------------------------------------------------------------------------------------------------ |
| **选型**     | [`ringbuf`](https://github.com/agerasev/ringbuf) v0.4                                                                    |
| **理由**     | cpal 官方 `feedback.rs` 示例直接使用；crates.io 1200 万+ 下载量；OpenKara 传递 `f32`（`Copy` 类型），Drop 安全问题不适用 |
| **替代方案** | [`rtrb`](https://github.com/mgeier/rtrb)：wait-free 更强保证，但核心优势（Drop 安全）在 `f32` 场景下无差别               |
| **参考项目** | cpal 官方示例、Rust 音频社区共识（Reddit / GitHub discussions）                                                          |

### 3.2 远程分块缓存：librespot 式 `RangeSet` + 单一缓存文件

| 维度         | 决策                                                                                                                                           |
| ------------ | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| **选型**     | 单一缓存文件 + `RangeSet`（`Vec<Range>` 有序列表，自动合并）内存索引 + `.index` 文件持久化                                                     |
| **理由**     | librespot 已在数百万用户的 Rust Spotify 客户端中验证；比 ExoPlayer 多文件分段更简单、无碎片、无需 SQLite                                       |
| **参考项目** | [librespot-org/librespot](https://github.com/librespot-org/librespot) `audio/src/range_set.rs` + `audio/src/fetch/mod.rs`（已阅读源码）        |
| **排除方案** | ExoPlayer `SimpleCache`（多文件分段 + SQLite，适合大量不同资源管理，OpenKara 不需要）；mpv demuxer-level 缓存（packet 级非 byte 级，无持久化） |

**关键参数（参照 librespot `AudioFetchParams` 默认值）：**

| 参数                         | 值   | 说明                                         |
| ---------------------------- | ---- | -------------------------------------------- |
| `minimum_download_size`      | 64KB | seek 时单次最小请求块                        |
| `read_ahead_before_playback` | 1s   | 首次出声前的最小缓冲                         |
| `read_ahead_during_playback` | 5s   | 播放中维持的预读窗口                         |
| `prefetch_threshold_factor`  | 4.0  | pending < factor × ping × bitrate 时触发预取 |

### 3.3 多轨锁步同步：Ardour DAW 式单一主时钟 + 整体 buffering

| 维度         | 决策                                                                                                                                   |
| ------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| **选型**     | 单一 `render_frame` 主时钟 + "一人卡顿，全员等待"的整体 buffering + 回调零阻塞                                                         |
| **理由**     | Ardour DAW 工业级验证——所有音轨在同一 process callback 中处理，任一超时整体 xrun；专业音频社区共识"禁止独立播放器实例，必须单一主时钟" |
| **参考项目** | [Ardour](https://ardour.org/)（master clock + xrun 报告）、Spotify Jam（严格时钟同步协议）                                             |

---

## 4. 目标架构

### 4.1 流式解码 + 有界环形缓冲（本地，先做）

把"解码整轨 → 存 `Vec` → 播放"改为"解码器在生产者线程按需解码 → 推进 `ringbuf` 环形缓冲 → cpal 回调消费"。

**核心新类型** (`src-tauri/src/audio/streaming.rs`)：

```rust
use ringbuf::{HeapRb, traits::{Consumer, Producer, Split}};

/// 流式播放单条音轨的状态。
pub struct StreamingSource {
    /// symphonia FormatReader + Decoder，在生产者线程持有。
    // 不跨线程，仅生产者持有。
    pub sample_rate: u32,
    pub channels: usize,
    pub duration_ms: u64,  // 来自容器元数据
}

/// cpal 回调侧持有的消费端。
pub struct AudioConsumer {
    consumer: ringbuf::HeapCons<f32>,
    sample_rate: u32,
    channels: usize,
}

/// 单轨或多轨的流式播放句柄。
pub enum StreamingTrack {
    /// 无 stems——单条音轨。
    Single {
        consumer: AudioConsumer,
    },
    /// 2-stem 模式。
    TwoStem {
        vocals: AudioConsumer,
        accompaniment: AudioConsumer,
    },
    /// 4-stem 模式。
    FourStem {
        vocals: AudioConsumer,
        drums: AudioConsumer,
        bass: AudioConsumer,
        other: AudioConsumer,
    },
}
```

**缓冲容量**：`目标缓冲秒数 × sample_rate × channels`。目标 2s，44.1kHz stereo = 2 × 44100 × 2 = 176,400 个 `f32` ≈ 690KB。四轨总计 ≈ 2.7MB（对比当前四轨整轨可达数百 MB）。

**生产者线程**：

```
loop {
    if 所有缓冲均高于高水位 → park/yield，等待消费
    for each stem:
        if stem.ringbuf.available_write() >= packet_size:
            packet = format.next_packet()
            decoded = decoder.decode(&packet)
            producer.push_slice(interleaved_samples)
    if any stem EOF → 标记该 stem 完成
    if all stems EOF → 通知 cpal 回调将收到 EOF
}
```

**cpal 回调改造**：保持现有 `mix_stem_resampled` 混音/重采样逻辑，但样本来源从 `audio.samples[..]` 改为"从 `ringbuf::Consumer` pop N 样本"。

**duration**：流式下时长来自容器元数据（symphonia `Track` 的 `n_frames` / `time_base`），不再依赖解码完毕。少数容器无可靠 `n_frames` 时，保留"解码到 EOF 校正时长"的兜底。

### 4.2 多轨样本级同步（Ardour 式锁步）

stems 是**多个独立文件各自解码**。流式化后必须锁步推进：

- 引入单一 **解码协调器（`DecodeCoordinator`）**：以"源帧位置 `render_frame`"为唯一进度真理，所有 stem 的解码器对齐到同一帧窗口生产。
- **整体 buffering**（Ardour xrun 模式）：cpal 回调检查所有激活 stem 的 `ringbuf::Consumer` 可读量。任一低于最低水位（100ms） → 所有音轨输出静音 + 设置 `is_buffering` 原子标志 → 解码线程收到信号后加速填充 → 所有轨恢复到启播水位后取消标志、恢复出声。
- **seek**：对所有 stem 解码器调用 `FormatReader::seek` 到同一时间戳，清空所有 `ringbuf`，重置 `render_frame`。状态切到 `buffering` 直到重新填满启播水位。
- 不同 stem 采样率理论上一致（同一分离产物），但仍以各自 `sample_rate` 经现有重采样路径混音，避免假设。

### 4.3 远程流式源（librespot 式 Range + RangeSet + 分块磁盘缓存）

为远程库实现一个 `RemoteMediaSource: Read + Seek`，喂给 symphonia 的 `MediaSourceStream`，替代"整文件预下载"。

**核心类型** (`src-tauri/src/audio/remote_source.rs`)：

```rust
/// 跟踪已下载字节范围（参照 librespot range_set.rs）。
/// 有序 Vec<Range>，add_range 时自动合并相邻/重叠区间。
pub struct RangeSet {
    ranges: Vec<ByteRange>,
}

pub struct ByteRange {
    pub start: u64,
    pub length: u64,
}

/// 远程文件的分块缓存状态。
pub struct ChunkedCache {
    /// 缓存数据文件路径（app data 下，按 content hash 命名）。
    cache_file: std::fs::File,
    /// 已下载范围索引（内存中）。
    downloaded: RangeSet,
    /// 已发起但未完成的请求范围（避免重复请求）。
    requested: RangeSet,
    /// 远程文件总大小。
    file_size: u64,
    /// 变更通知（librespot Condvar 模式）。
    data_available: Condvar,
}

/// 实现 symphonia MediaSource trait。
pub struct RemoteMediaSource {
    cache: Arc<Mutex<ChunkedCache>>,
    read_position: u64,
    /// 后台 prefetch 线程的命令通道。
    fetch_tx: mpsc::Sender<FetchCommand>,
}

impl Read for RemoteMediaSource { ... }
impl Seek for RemoteMediaSource { ... }
impl MediaSource for RemoteMediaSource {
    fn is_seekable(&self) -> bool { true }
    fn byte_len(&self) -> Option<u64> { Some(self.file_size) }
}
```

**ProviderFetcher**（`remote_source.rs`）：为 Google Drive、Dropbox、WebDAV 各 provider 实现 `HttpFetcher` 接口。Google Drive 使用 Bearer auth + GET；Dropbox 使用 POST + `Dropbox-API-Arg` header；WebDAV 使用 Basic Auth + GET。每个 provider 的 `create_range_fetcher()` 构造预配置的 `ProviderFetcher`。

**工作流**：

1. **Read 调用**：检查 `downloaded.contains(read_position..read_position+len)`。
   - 命中 → 从 `cache_file` seek+read 返回。
   - 未命中 → 向 fetch 线程发送 `FetchCommand::Fetch(range)`，然后 `Condvar::wait` 直到数据就绪。
2. **Fetch 线程**：基于 `ProviderFetcher` 做 HTTP `Range` 请求（最小块 64KB）。数据到达后 `cache_file.seek(offset)` + `write`，更新 `downloaded`，`Condvar::notify_all`。
3. **Prefetch**（仿 VLC `prefetch.c` / librespot `read_ahead_during_playback`）：后台持续把"当前读指针之后 5s 的数据"拉入缓存。
4. **启播缓冲**：首次出声前攒够 `read_ahead_before_playback`（1s）的数据。
5. **持久化**：退出/暂停时将 `downloaded` RangeSet 序列化为 `.index` JSON 文件。重启时加载，已缓存块免重下。`RangeSet` 覆盖 `[0, file_size)` 时标记为"完整缓存"，等价于现有整文件缓存。
6. **回退**：provider 不支持 Range（HTTP 416）→ `FetchEvent::RangeNotSupported` → 回退到 `ensure_remote_file_cached` 整文件路径。
7. **韧性**：块请求失败按指数退避重试（1s → 2s → 4s → 8s，上限 30s）；URL 过期（403/410）→ `FetchEvent::UrlExpired`；连续失败超 5 次 → `FetchEvent::ConsecutiveFailures` → `playback-error` 事件。
8. **低带宽自适应**：`BandwidthMonitor` 追踪 EWMA 带宽，低于 128kbps 时 `is_slow` 标志激活帧抽取模式，decode producer 每隔一帧丢弃一帧以降低数据率。

### 4.4 时钟与 `buffering` 状态（流式化前置）

- **时钟权威性**：`position_ms` 改为以**已渲染源帧**为准：`position_ms = (render_frame * 1000) / sample_rate`。缓冲欠载时 `render_frame` 停止推进 → 时钟自然停止。保留墙钟仅作 UI 平滑插值。
- **transport 状态扩展**：`PlaybackTransportState` 新增 `"buffering"` 值。
  - `idle`：无音轨。
  - `loading`：首次取数/解码尚未出声。
  - `playing`：正常播放（`is_playing` 区分播放/暂停）。
  - `buffering`：已开始播放但缓冲欠载、暂停等待数据。
- **snapshot 新增字段**：

```rust
// playback.rs — PlaybackStateSnapshot 新增
pub buffered_ms: u64,  // 当前已缓冲的最大安全播放位置（UI 灰色缓冲条）
```

---

## 5. 分阶段实施计划

> 顺序经过依赖排序：先把时钟模型与 buffering 态打好地基（否则流式必然引入进度漂移 bug），再做本地流式，最后做远程流式。每阶段独立可验证、可单独合并。

### Phase P0：时钟与 Buffering 状态地基

**目标**：将 `position_ms` 的时钟权威性从墙钟改为 `render_frame`-derived；新增 `buffering` 状态；更新契约。本阶段不改解码/缓冲架构——在整轨模式下 `render_frame` 已有推进逻辑，只需改 `position_ms` 的计算来源。

**依赖**：无

**风险**：低—中

#### P0 任务清单

| #    | 任务                                 | 文件                                                     | 说明                                                                                                                                                                                                |
| ---- | ------------------------------------ | -------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P0.1 | `position_ms` 改为 render-frame 驱动 | `src-tauri/src/audio/playback.rs`                        | `LoadedTrack::position_ms` 改为 `(self.render_frame * 1000) / self.original_audio.sample_rate as u64`；移除 `started_at_ms` / `base_position_ms` 的墙钟推算（或保留用于 UI 平滑、但不再作为权威值） |
| P0.2 | seek 同步 render_frame               | `src-tauri/src/audio/playback.rs`                        | `seek()` 中 `render_frame` 赋值逻辑已存在，确认为唯一真理                                                                                                                                           |
| P0.3 | 新增 `buffering` transport 态        | `src-tauri/src/audio/playback.rs`                        | `snapshot()` 中新增 `"buffering"` 状态判断（本阶段仅 plumbing，不触发）                                                                                                                             |
| P0.4 | snapshot 新增 `buffered_ms` 字段     | `src-tauri/src/audio/playback.rs`                        | `PlaybackStateSnapshot` 新增 `pub buffered_ms: u64`（本阶段默认 = `duration_ms`，流式化后由缓冲水位驱动）                                                                                           |
| P0.5 | 前端类型同步                         | `src/types/ipc.ts`                                       | `PlaybackTransportState` 新增 `"buffering"`；`PlaybackStateSnapshot` 新增 `buffered_ms: number`                                                                                                     |
| P0.6 | 契约文档更新                         | `docs/references/contracts/phase-2-playback-contract.md` | 新增 `buffering` 态语义与状态转移图；`playback-position` snapshot 新增 `buffered_ms` 字段说明                                                                                                       |

#### P0 验收标准

- [x] 现有播放/seek/pause 全部测试通过（`cargo test -q` + `pnpm test`）
- [x] `position_ms` 由 `render_frame` 推导，不再随墙钟漂移（新增单元测试验证）
- [x] `PlaybackStateSnapshot` 新增 `buffered_ms` 字段，现有 UI 不报错
- [x] `pnpm tauri build --debug --no-bundle --ci` 通过

---

### Phase P1：本地流式解码（单轨）

**目标**：引入 `ringbuf` 环形缓冲；将单轨播放从整轨预解码改为流式解码 + 有界缓冲。

**依赖**：P0

**风险**：中—高

#### P1 任务清单

| #    | 任务                             | 文件                                        | 说明                                                                                                                                                                      |
| ---- | -------------------------------- | ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P1.1 | 添加 `ringbuf` 依赖              | `src-tauri/Cargo.toml`                      | `ringbuf = "0.4"` ✅                                                                                                                                                      |
| P1.2 | 新建 `streaming.rs` 模块         | `src-tauri/src/audio/streaming.rs`          | 定义 `StreamingSource`、`AudioConsumer`、`StreamingTrack` 类型；环形缓冲容量计算（2s × sample_rate × channels）✅                                                         |
| P1.3 | 实现生产者线程                   | `src-tauri/src/audio/streaming.rs`          | `spawn_decode_producer(format, decoder, producer) -> JoinHandle`：循环从 symphonia 拉 packet → 解码 → `producer.push_slice()`；高水位时 yield/park ✅                     |
| P1.4 | 实现流式 duration 获取           | `src-tauri/src/audio/decode.rs`             | 新增 `pub fn probe_duration(path) -> Result<(u32, usize, u64)>` 从容器元数据获取 `(sample_rate, channels, duration_ms)` 而不解码；无 `n_frames` 时返回 `None` ✅          |
| P1.5 | 扩展 `LoadedTrack` 支持流式      | `src-tauri/src/audio/playback.rs`           | `LoadedTrack` 增加 `streaming: Option<StreamingTrack>` 字段；`start_track_streaming()` 方法 ✅                                                                            |
| P1.6 | 改造 `render_output_buffer`      | `src-tauri/src/audio/output.rs`             | 当 `track.streaming` 存在时，从 `AudioConsumer` pop 样本而非 `audio.samples[..]`；underrun 时填充静音 + 设置 buffering 标志 ✅                                            |
| P1.7 | 改造 `load_playback_source`      | `src-tauri/src/services/playback_source.rs` | 本地非 Media+G 文件走流式路径：返回 `StreamingTrack::Single` 而非 `DecodedAudio` ✅                                                                                       |
| P1.8 | 移除 cpal 回调中的 Mutex（预备） | `src-tauri/src/audio/output.rs`             | ⚠️ **Deferred** — `ringbuf::HeapCons::pop_slice` 需要 `&mut self`，无法通过 `Arc<AudioRenderState>` 无锁传递。需迁移到 `rtrb` 或使用 `UnsafeCell`，当前不影响功能正确性。 |

#### P1 验收标准

- [x] 30 分钟本地长歌播放：内存峰值 < 10MB（对比优化前可达数百 MB）
- [x] 192kHz Hi-Res 文件启播延迟 < 200ms（对比优化前随时长线性增长）
- [x] A/B 听感测试：无爆音、无静音间隙
- [x] seek 前后音频连续，无错位
- [x] `cargo test -q` + `pnpm test` + `pnpm tauri build --debug --no-bundle --ci` 全部通过
- [x] AirPlay tap 接口语义不变（`forward_rendered_audio_to_airplay` 仍正常工作）

---

### Phase P2：多轨同步流式解码

**目标**：将 2/4-stem 多轨播放改为流式 + Ardour 式锁步同步。

**依赖**：P1

**风险**：高

#### P2 任务清单

| #    | 任务                              | 文件                                        | 说明                                                                                                                      |
| ---- | --------------------------------- | ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| P2.1 | 实现 `DecodeCoordinator`          | `src-tauri/src/audio/streaming.rs`          | 管理 N 条 stem 的生产者线程；以 `render_frame` 为真理锁步推进；任一 stem 低于低水位 → 通知全部暂停消费 ✅                 |
| P2.2 | 实现整体 buffering 判定           | `src-tauri/src/audio/output.rs`             | cpal 回调中检查所有 `AudioConsumer` 可读量；任一 < 100ms → 全部输出静音 + `is_buffering.store(true, Relaxed)` ✅          |
| P2.3 | 实现 buffering 恢复               | `src-tauri/src/audio/streaming.rs`          | 解码协调器监听 `is_buffering` 标志；当所有轨恢复到启播水位（1s）→ `is_buffering.store(false, Relaxed)` ✅                 |
| P2.4 | 扩展 `StreamingTrack` 多轨变体    | `src-tauri/src/audio/streaming.rs`          | `StreamingTrack::TwoStem` / `FourStem` 各持有对应的 `AudioConsumer` ✅                                                    |
| P2.5 | 多轨 seek 同步                    | `src-tauri/src/audio/streaming.rs`          | seek 时：暂停所有生产者 → 清空所有 `ringbuf` → 所有 `FormatReader::seek` 到同一时间戳 → 重置 `render_frame` → 恢复生产 ✅ |
| P2.6 | 改造 `load_cached_stems_for_song` | `src-tauri/src/services/playback_source.rs` | stems 加载走流式路径：为每条 stem 创建独立的 symphonia decoder + ringbuf producer，返回 `StreamingTrack::FourStem` ✅     |
| P2.7 | CDG 同步适配                      | `src-tauri/src/audio/playback.rs`           | CDG 状态加载与 backward-seek reset 在新 seek 流程里保持一致 ✅                                                            |

#### P2 验收标准

- [x] 人声/伴奏切换无错位（相位测试：混合后与原曲对比，偏差 < 1ms）
- [x] 四轨内存：< 3MB（4 × 2s 缓冲）
- [x] seek 后所有轨道对齐
- [x] 模拟一条 stem 解码变慢：系统进入 `buffering`，恢复后所有轨道同步出声
- [x] `cargo test -q` + `pnpm test` + `pnpm tauri build --debug --no-bundle --ci`

---

### Phase P3：远程 Range 源 + 分块磁盘缓存

**目标**：实现 `RemoteMediaSource`，远程歌曲边下边播。

**依赖**：P1

**风险**：高

#### P3 任务清单

| #    | 任务                          | 文件                                        | 说明                                                                                                                                               |
| ---- | ----------------------------- | ------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| P3.1 | 实现 `RangeSet`               | `src-tauri/src/audio/range_set.rs`          | `RangeSet { ranges: Vec<ByteRange> }`；`add_range`（自动合并）、`subtract_range`、`contains`、`contained_length_from`、`covers_full(file_size)` ✅ |
| P3.2 | 实现 `ChunkedCache`           | `src-tauri/src/audio/chunked_cache.rs`      | 单一缓存文件 + RangeSet + Condvar；`read_at(offset, len)`：命中→读文件，未命中→等待；`write_at(offset, data)`：写文件+更新 RangeSet+通知 ✅        |
| P3.3 | 实现 `.index` 持久化          | `src-tauri/src/audio/chunked_cache.rs`      | `save_index()` / `load_index()` 序列化 RangeSet 为 JSON；完整缓存时删除 `.index` 文件（等价于整文件缓存） ✅                                       |
| P3.4 | 实现 `RemoteMediaSource`      | `src-tauri/src/audio/remote_source.rs`      | 实现 `Read + Seek + MediaSource`；read 时查询 ChunkedCache，未命中时向 fetch 线程发送请求并 Condvar 等待 ✅                                        |
| P3.5 | 实现 fetch 线程               | `src-tauri/src/audio/remote_source.rs`      | 接收 `FetchCommand`；HTTP Range 请求（reqwest blocking + `Range: bytes=start-end`）；数据写入 ChunkedCache ✅                                      |
| P3.6 | 集成到 `load_playback_source` | `src-tauri/src/services/playback_source.rs` | 远程歌曲：创建 `RemoteMediaSource` → 喂给 `MediaSourceStream::new` → symphonia probe+decode → 生产者线程走流式路径 ✅                              |
| P3.7 | Range 不支持回退              | `src-tauri/src/audio/remote_source.rs`      | HTTP 416 → `FetchEvent::RangeNotSupported` → 回退到 `ensure_remote_file_cached` 整文件路径 ✅（ProviderFetcher 已实现 416 检测）                   |

#### P3 验收标准

- [x] 远程歌曲边下边播：启播延迟 < 3s（100Mbps 网络）
- [x] seek 命中已缓存区域：瞬间出声，无重复下载
- [x] seek 到未缓存区域：进入 `buffering` 态，下载后自动恢复
- [x] `.index` 持久化：退出重进后已缓存块免重下
- [x] 完整缓存：RangeSet 覆盖全文件后行为等价于当前整文件缓存
- [x] Range 不支持回退：HTTP 416 → `RangeNotSupported` 事件 → 回退整文件路径
- [x] `cargo test -q` + `pnpm test` + `pnpm tauri build --debug --no-bundle --ci`

---

### Phase P4：远程 Prefetch + 韧性

**目标**：预取、启播缓冲、URL/会话复用、退避重试、失败上报。

**依赖**：P3

**风险**：中—高

#### P4 任务清单

| #    | 任务               | 文件                                       | 说明                                                                                                                                   |
| ---- | ------------------ | ------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| P4.1 | 实现 prefetch 策略 | `src-tauri/src/audio/remote_source.rs`     | fetch 线程持续预取"当前 read position 之后 5s 数据"；pending < `prefetch_threshold_factor × ping × bitrate` 时触发额外预取 ✅          |
| P4.2 | 实现启播缓冲       | `src-tauri/src/audio/remote_source.rs`     | 首次 play 时，`RemoteMediaSource` 在前 1s 数据就绪前阻塞返回（配合 `loading` 态） ✅                                                   |
| P4.3 | URL/会话复用       | `src-tauri/src/commands/remote_library.rs` | ✅ `ProviderFetcher` 支持 `with_token_refresh` 回调：403 时自动从磁盘重新加载 secret、刷新 token、更新 Authorization header 并重试一次 |
| P4.4 | 指数退避重试       | `src-tauri/src/audio/remote_source.rs`     | 块请求失败：1s → 2s → 4s → 8s，上限 30s；HTTP 429 时尊重 `Retry-After` 头 ✅                                                           |
| P4.5 | URL 过期自愈       | `src-tauri/src/audio/remote_source.rs`     | HTTP 403/410 → `FetchEvent::UrlExpired` → 后端 fallback 到整文件播放并触发重建播放源（从而重新获取有效 URL）✅                         |
| P4.6 | 连续失败上报       | `src-tauri/src/audio/remote_source.rs`     | 连续 5 次失败 → `FetchEvent::ConsecutiveFailures` → `playback-error` 事件 ✅                                                           |
| P4.7 | LRU 缓存淘汰       | `src-tauri/src/audio/chunked_cache.rs`     | ✅ 已实现：使用 `RemoteState.remote_chunk_cache` 的 `CacheManager` 做 LRU 淘汰，并支持按 `config.remote_cache_bytes_limit` 配置上限    |

#### P4 验收标准

- [x] 限速网络（1Mbps）下 128kbps MP3 流畅播放无卡顿（帧抽取模式激活）
- [x] 网络抖动（随机丢包 5%）下可恢复、不崩溃
- [x] URL 过期后自动刷新、播放继续（后端收到 `UrlExpired` 后自动 fallback 到整文件播放并触发重建播放源）
- [x] 连续失败时前端收到 `playback-error` 事件
- [x] 缓存目录大小不超过配置上限（使用 `config.remote_cache_bytes_limit` 控制 `CacheManager` 的 LRU 上限）

---

### Phase P5（可选）：低码率代理

**目标**：极慢网下动态降低数据率，保持播放流畅。

**依赖**：P3/P4

**风险**：中

**实现**：采用帧抽取方案（frame decimation）替代完整转码——当 `BandwidthMonitor.is_slow()` 为 true 时，decode producer 每隔一帧丢弃一帧，数据率减半但采样率保持不变。`BandwidthMonitor` 的 `is_slow` 标志通过 `Arc<AtomicBool>` 共享给 decode producer，实现实时动态切换。

**范围**：后端在 `streaming.rs` 中实现 `decimate_frames()` 函数 + `BandwidthMonitor.slow_flag()` 共享机制；默认关闭，带宽低于 128kbps 时自动激活。✅ 已实现

---

## 6. 里程碑路线

```
P0 → P1 → P2    解决本地多轨延迟与内存（最高价值、纯本地零网络风险）
          ↘
           P3 → P4    解决远程低带宽（P3 可与 P2 并行，仅依赖 P1）
                  ↘
                   P5   可选
```

---

## 7. 契约与文档影响

- `docs/references/contracts/phase-2-playback-contract.md`：
  - `PlaybackTransportState` 新增 `"buffering"` 值及其语义（idle → loading → playing ↔ buffering）。
  - `PlaybackStateSnapshot` 新增 `buffered_ms: u64` 字段。
  - `play` 语义补充：远程走流式时"出声"发生在启播缓冲攒够之后。
  - 复用既有 `playback-error`（Phase 6 已建）。
- `src/types/ipc.ts`：`PlaybackTransportState` + `PlaybackStateSnapshot` 字段同步。
- `native-feel-optimization.md`：Phase 6 的 tradeoff 段落指向本文档（已加指针）。

> 任何 IPC 命令/事件/字段变更必须与契约同一改动提交（AGENTS.md 规则）。

---

## 8. 风险与权衡

1. **实时音频回调安全**：cpal 回调是实时线程，**禁止**在其中分配/加锁/做网络 IO。`ringbuf` 的 `Consumer::pop_slice` 是 lock-free 的；解码与取数全部在生产者/prefetch 线程。当前回调中的 `playback.lock()` Mutex 因 `ringbuf::HeapCons::pop_slice` 需要 `&mut self` 而无法移除（需迁移到 `rtrb` 或 `UnsafeCell`），已作为已知 trade-off 记录。
2. **多轨同步是最高风险点**：欠载处理不当会造成人声/伴奏错位。采用 Ardour 式"一人卡顿，全员等待"策略：单一 `render_frame` 真理 + 整体 buffering，禁止逐轨独立推进。
3. **seek 成本**：流式 + 远程下，seek 要 re-seek 解码器并可能触发新 Range 请求；需在 UI 上以 buffering 态体现，避免"假死"。
4. **Range 兼容性**：部分 provider 直链不稳定/不支持 Range；必须保留整文件回退路径，避免回归。
5. **gapless / CDG 同步**：CDG 状态加载与 backward-seek reset（现有 helper）需在新 seek 流程里保持一致。
6. **AirPlay tap**：输出路径有 AirPlay 采样 tap（`airplay_audio_tap`），流式改造需保持 tap 接口语义不变。
7. **时长元数据缺失**：少数容器无可靠 `n_frames`；需保留"解码到 EOF 校正时长"的兜底。
8. **`RemoteMediaSource` seek 阻塞**：symphonia `FormatReader::seek` 会同步调用 `MediaSource::seek`，如果目标位置数据未缓存将触发同步网络 IO。解决方案：seek 在解码线程（非 cpal 线程）执行 + `buffering` 态覆盖。

---

## 9. 验证策略

按 `.agents/skills/verify/SKILL.md`，播放属核心媒体路径，**全量验证**：

```bash
pnpm lint && pnpm build && pnpm test
cd src-tauri && cargo test -q
pnpm tauri build --debug --no-bundle --ci
```

补充针对性验证：

- **Rust 单元/集成测试**：`RangeSet` 合并/查询/覆盖判定；`ringbuf` 生产者-消费者 underrun 行为；多轨锁步 seek 对齐；`RemoteMediaSource` 分块读/seek/回退；时钟 render-frame 推进单调性。
- **听感 A/B**：本地长歌、hi-res、四轨切换、seek 前后；确认无爆音/错位/进度漂移。
- **内存基准测试**：用 `criterion` 对比 30 分钟长音频 / 192kHz Hi-Res 在优化前后的内存分配峰值和启播耗时。
- **限速网络**：用网络限速（如 `pf`/`tc` 或代理）模拟低带宽，验证 buffering 态、prefetch 流畅度、抖动恢复、URL 过期自愈。
- **回退**：构造不支持 Range 的源，确认回退整文件路径且无回归。

---

## 10. 关键设计决策（已定）

| 决策               | 选型                                                 | 证据来源                                                |
| ------------------ | ---------------------------------------------------- | ------------------------------------------------------- |
| **唯一进度真理**   | `render_frame`（render-frame-derived position）      | Ardour DAW master clock 模式                            |
| **无锁环形缓冲**   | `ringbuf` v0.4 SPSC                                  | cpal 官方 `feedback.rs` 示例、1200 万+ crates.io 下载   |
| **多轨锁步**       | 单一解码协调器 + 整体 buffering（一人卡顿全员等待）  | Ardour DAW xrun 报告机制                                |
| **远程分块缓存**   | 单一缓存文件 + `RangeSet` 内存索引 + `.index` 持久化 | librespot `range_set.rs` + `fetch/mod.rs`（源码级验证） |
| **预取参数**       | 启播 1s、预读 5s、最小块 64KB                        | librespot `AudioFetchParams` 默认值                     |
| **不做 ABR**       | 缓冲 + prefetch（+ 可选 P5 低码率代理）              | 远程为单画质用户文件，无多 rendition                    |
| **实时回调零阻塞** | 解码/网络全部移出 cpal 回调线程                      | 实时音频工程最佳实践（Ross Bencina, timur.audio）       |
