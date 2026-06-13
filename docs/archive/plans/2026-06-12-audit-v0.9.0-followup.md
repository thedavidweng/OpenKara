# 验收回归修复计划（post audit-v0.9.0）

**Status:** Completed · 2026-06-12  
**Baseline:** audit-v0.9.0 execution on uncommitted working tree

## 背景

上一轮 agent 执行 `docs/plans/audit-v0.9.0.md` 后用户验收发现 6 类问题。本 follow-up 针对验收项修复，不重复原 audit 全量 backlog。

## 已完成项

### 1. 歌词可读性

- [PlaybackStage.tsx](../../src/components/Playback/PlaybackStage.tsx)：封面 ambience 遮罩加深（`brightness-[0.75]` + 更强径向/线性渐变）。
- [LyricLine.tsx](../../src/components/Lyrics/LyricLine.tsx)：standard 模式改用白色透明度梯度（`text-white/45`、`text-white/50`）。
- [LyricsPanel.tsx](../../src/components/Lyrics/LyricsPanel.tsx)：移除 `mixBlendMode: plus-lighter`。

### 2. Hover 高亮

- [LyricLine.tsx](../../src/components/Lyrics/LyricLine.tsx)：`group-hover/line:bg-white/10` 圆角背景，替代下划线。

### 3. 播放状态链

- [output.rs](../../src-tauri/src/audio/output.rs)：多 stem 共享源帧 budget（R4 重做）；自然结束 `finalize_streaming_natural_end`。
- [streaming.rs](../../src-tauri/src/audio/streaming.rs)：EOF 感知低/高水位；`all_eof_and_drained()`。
- [playback.rs](../../src-tauri/src/audio/playback.rs)：`duration_ms == 0` 不钳制；snapshot 报 `None`；EOF 回填时长。
- [player-store.ts](../../src/stores/player-store.ts)：移除 F10 反向守卫；跨 webview `playingSinceMs` 重定基。

### 4. 本地歌词假死

- [fetch.rs](../../src-tauri/src/lyrics/fetch.rs)：本地优先（embedded/sidecar → online）。
- [lrclib.rs](../../src-tauri/src/lyrics/lrclib.rs) / [lrcapi.rs](../../src-tauri/src/lyrics/lrcapi.rs)：HTTP 3s connect / 6s timeout。
- [commands/lyrics.rs](../../src-tauri/src/commands/lyrics.rs) + [cache/lyrics.rs](../../src-tauri/src/cache/lyrics.rs)：`LyricsSource::Absent` 负缓存。

### 5. remote_source 警告

- [remote_source.rs](../../src-tauri/src/audio/remote_source.rs)：抽取 `handle_update_position` 闭包，排空分支与独立分支共用预取逻辑。

## 验证

```bash
pnpm lint && pnpm build && pnpm test
cd src-tauri && cargo test -q
```

全部通过（2026-06-12）。

## 手动验收清单

- [ ] 亮色封面下非活动歌词可读
- [ ] 播放中按钮为暂停、歌词随播放滚动
- [ ] 曲目自然结束触发下一首
- [ ] hover 为高亮无下划线
- [ ] 仅有本地歌词的曲目秒开
