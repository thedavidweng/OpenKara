# OpenKara 原生感优化实施记录

> Status: implemented.
>
> 目标：消除 OpenKara 的"网页应用"感，使其在交互、视觉和窗口行为上与原生桌面应用一致。
> 本记录保留原计划、实现结果和剩余手动验证项。

---

## 背景调研摘要

### 技术栈

- **Frontend**: React + Tailwind CSS + Vite，运行在 Tauri 2 WebView 中
- **Backend**: Rust + Tauri 2.11.1（`tauri` crate features 含 `"unstable"`）
- **macOS Shell**: 已有自定义 Objective-C 桥接（`src-tauri/src/macos/window_shell.m`），处理 traffic light 布局、标题栏隐藏、full-size content view

### 已发现的"网页感"问题

| 问题                                    | 严重程度 | 当前实现                                                                                                           | 影响范围   |
| --------------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------ | ---------- |
| **启动窗口闪烁**                        | **高**   | 窗口创建后立即显示，CSS/React 首帧渲染前 body 是默认白色；等待 IPC（`getLibraryRegistry`）再渲染 UI 期间窗口已可见 | 所有平台   |
| `cursor: pointer` 残留                  | 中       | 7 处显式声明（滑块、进度条、歌词行、设置 toggle）                                                                  | 桌面端交互 |
| Tooltip / ContextMenu 限制在 WebView 内 | 中       | React Portal → DOM 元素，无法超出窗口边界；边缘处理逻辑已实现须验证                                                | 边缘场景   |
| 窗口 resize 可能卡顿                    | 低       | 未做 AppKit animated resize 优化                                                                                   | macOS      |

---

## Phase 1: 移除 `cursor: pointer`（前端，低风险）

### 现状

仓库中绝大多数交互元素已使用 `.motion-icon-button`（无显式 cursor），但仍残留 7 处：

| 文件                                                       | 位置                                   | 说明                                                                    |
| ---------------------------------------------------------- | -------------------------------------- | ----------------------------------------------------------------------- |
| `src/styles/globals.css:274`                               | `.native-slider`                       | 音量/进度条轨道                                                         |
| `src/styles/globals.css:286`                               | `.native-slider::-webkit-slider-thumb` | 滑块 thumb                                                              |
| `src/styles/globals.css:310`                               | `.audio-level-slider:disabled`         | disabled 时 `cursor: default` — 上层移除 pointer 后此行为冗余，一并删除 |
| `src/components/Player/SeekBar.tsx:84`                     | 进度条轨道 div                         | `cursor-pointer` class                                                  |
| `src/components/Player/VolumeSliders.tsx:407`              | 图标按钮                               | 条件 `cursor-pointer` / `cursor-default`                                |
| `src/components/Lyrics/LyricLine.tsx:113`                  | 可点击歌词行                           | 三元 `isSeekable ? "cursor-pointer" : "cursor-default"`                 |
| `src/components/Settings/SettingsGeneralSection.tsx:14,34` | 2 处 label                             | `cursor-pointer` class（checkbox toggle 的 label）                      |

### 实施结果

已直接移除显式 cursor 声明。桌面应用可点击性通过 hover 状态、颜色变化、背景高亮传达，无需 cursor 变化。

- `globals.css`：
  - `.native-slider` 和 `::-webkit-slider-thumb` 的 `cursor: pointer` 删除
  - `.audio-level-slider:disabled` 的 `cursor: default` 删除
- `SeekBar.tsx`：删除 `cursor-pointer` class
- `VolumeSliders.tsx`：删除条件 `cursor-pointer` / `cursor-default` 三元式
- `LyricLine.tsx`：删除整行条件 class（`cursor-pointer` 和 `cursor-default` 都删），回归默认 `auto`
- `SettingsGeneralSection.tsx`：2 处 `cursor-pointer` 删除

### 测试影响

- `LyricLine.test.tsx` 已有 `expect(markup).not.toContain("cursor-pointer")`，修改后全部行都不含 `cursor-pointer`，测试继续通过
- `pnpm test` 无回归

### 工作量

~15 分钟

---

## Phase 2: Tooltip / ContextMenu 边缘截断检查（前端，手动审计）

### 现状

**代码已完整实现边缘处理逻辑**，不是空白区域：

| 文件                                                    | 已实现的功能                                                                                                                |
| ------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `src/components/Overlay/Tooltip.utils.ts:51-66`         | `getTooltipPosition` 对左右边缘用 `Math.min/max` clamp（`TOOLTIP_VIEWPORT_PADDING_PX = 8`），对顶部空间不足时自动翻转到底部 |
| `src/components/Library/context-menu-position.ts:12-33` | `getContextMenuPosition` 对所有四个方向 clamp（`VIEWPORT_PADDING = 8`）                                                     |
| `src/components/Library/ContextMenu.tsx:230-235`        | `SubMenu` 在右侧空间不足时自动向左展开                                                                                      |

### 验证任务

此 phase 从"编码"降级为**手动 audit + 验证**，需在运行应用后确认无截断问题：

1. 构建 dev 版本，窗口缩至最小尺寸（680×800），在**四角**右键触发 ContextMenu，确认菜单完整可见
2. 在**右下角**触发 Tooltip，确认 tooltip 被 clamp 到 viewport 内且完整可见
3. 在**右侧 margin** 上触发含子菜单的 ContextMenu，确认子菜单向左展开
4. 确认关闭行为（点击外部 → close，ESC → close）无闪烁/延迟

若有实际截断发现再针对性修复，否则标记为已验证。

### 方案 B（暂缓，仅记录）：真正的原生浮动窗口

用 macOS `NSPopover` 或独立 `NSWindow` 创建 Tooltip/ContextMenu，可超出主窗口边界。Tauri 2 无现成 API，需要 Rust/ObjC 层创建 Child Window 并同步事件。跨平台成本过高，暂缓。

### 工作量

~30 分钟（手动测试 + 截图记录）

---

## Phase 3: 窗口 Resize 抗卡顿（macOS-only，文档阶段）

### 背景

WebKit 在 AppKit animated window resize 期间暂停绘制，导致窗口缩放时出现可见卡顿。修复思路：swizzle `NSWindow.setFrame:display:animate:`，将 animated resize 替换为隐式 Core Animation 动画。

### 现状

`window_shell.m` 已有 traffic light 布局、titlebar 隐藏、full-size content view 等 macOS 窗口优化，但未处理 resize 绘制同步。

### 方案（暂缓执行，代码仅供参考）

```objc
// 注意：以下代码**不可直接编译**，仅为思路示意。
// 实际实现应：
// 1. 使用 method_setImplementation 而非 method_exchangeImplementations
// 2. 目标类应为 WRY 内部使用的 NSWindow 子类（而非 NSWindow 本身），避免影响系统窗口
// 3. 考虑用 +[CATransaction begin/commit] 包裹而非 NSAnimationContext

static IMP s_original_setFrame = NULL;

static void replacement_setFrame(id self, SEL _cmd, NSRect frame) {
    // 拦截 animated 调用，用 CA 代替
    // 需要辨别的 selector：setFrame:display:animate:
    // 当 animate=YES 时，包在 [CATransaction begin] / [CATransaction commit] 中执行
    // 当 animate=NO 时，直接透传
}
```

### 风险

- **Tauri 升级兼容性**：WRY 的 NSWindow 子类名或行为可能变化
- **副作用**：可能影响全屏切换、窗口恢复等 Tauri 内部逻辑
- 需要先建立 resize 性能基准（录屏对比）

### 工作量（预估）

~3-4 小时（含测试和回归）

---

## Phase 4: 启动窗口闪烁修复（已实现）

### 问题描述

启动时窗口在 WebView 完成首帧渲染前就已可见，用户看到：

1. 窗口框架出现，body 默认**白色背景**（浏览器默认）
2. ~50-200ms 后，CSS bundle 加载完成，背景变为 #121212（深色）
3. 再等待 `useAppStartupRuntime` 中 `getLibraryRegistry` IPC 返回，`App.tsx` 才从 `return null` 过渡到渲染 `LibrarySetup` 或 `AppLayout`
4. 内容"跳"进窗口

### 根因

1. `index.html` 无内联 CSS，body 默认白色 → CSS bundle 加载后才变深色
2. `tauri.conf.json` 未设 `visible: false`，窗口立即可见
3. App 首屏依赖 `getLibraryRegistry()` IPC 返回值，渲染前有不可避免的空隙
4. 没有机制保证 WebView 首帧渲染完成后才显示窗口

### 实施结果

#### 4.1 内联 CSS 抹平白屏

`index.html` 已内联深色背景，消除 CSS bundle 加载前的白色闪烁：

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <link rel="icon" type="image/svg+xml" href="/vite.svg" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>OpenKara</title>
    <style>
      body {
        background-color: #121212;
        margin: 0;
      }
    </style>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

#### 4.2 `visible: false` + `window_ready` IPC

`tauri.conf.json` 已将主窗口设为初始不可见并居中：

```json
{
  "label": "main",
  "title": "OpenKara",
  "width": 1280,
  "minWidth": 680,
  "height": 800,
  "resizable": true,
  "visible": false,
  "center": true
}
```

Rust 层已新增 `window_ready` 命令，让前端通知后端窗口就绪：

```rust
// src-tauri/src/commands/window_shell.rs
#[tauri::command]
pub fn window_ready(window: tauri::WebviewWindow) -> CommandResult<()> {
    let _ = window.show();
    Ok(())
}
```

前端层已在 `libraryReady` 变为非空、实际 UI 准备渲染后调用 `window_ready`。
`window_ready` 不在 `App.tsx` 首次 `useEffect([], [])` 中发送，因为首次渲染时
`libraryReady === null`，组件返回 `null`（空白）。正确的时机是**首次渲染出实际 UI** 之后：

```typescript
// src/App.tsx
function App({ initialLibraryReady = null }: AppProps) {
  const [libraryReady, setLibraryReady] = useState<boolean | null>(
    initialLibraryReady,
  );
  useAppStartupRuntime(libraryReady, setLibraryReady);
  useAppRuntime(libraryReady === true);
  const [windowShown, setWindowShown] = useState(false);

  useAppReadyRuntime(libraryReady, setWindowShown);
  // ...

  // ... render ...
}

// 单独 hook
// src/runtime/app-runtime.ts
export function useAppReadyRuntime(
  libraryReady: boolean | null,
  setWindowShown: (shown: boolean) => void,
) {
  useEffect(() => {
    if (libraryReady !== null && !windowShown) {
      // libraryReady 已确定（true 或 false），组件即将渲染实际 UI
      // 用 requestAnimationFrame 等待当前帧 paint 完成
      requestAnimationFrame(() => {
        void invoke("window_ready");
        setWindowShown(true);
      });
    }
  }, [libraryReady, setWindowShown]);
}
```

**基本原理**：

- 组件在 `libraryReady` 变为非 null 后重新渲染（`LibrarySetup` 或 `AppLayout`）
- `useEffect` 在 React commit + browser paint 之前触发
- `requestAnimationFrame` 回调在下一帧 paint **之前**调用 → 触发 `window.show()`
- 窗口显示时，至少上一帧的内容已在屏幕上

#### 4.3 模型校验启动优化

额外实现了模型 verified manifest：

1. 模型下载或首次完整 SHA-256 校验通过后，写入 `<filename>.verified.json`
2. 后续启动如果 manifest 的文件名、pinned SHA、文件大小和修改时间都匹配，则不再读取整个 ONNX 文件
3. manifest 缺失或不匹配时才重新执行完整 SHA-256 校验，并在通过后重写 manifest

这避免每次启动同步读取 300MB+ ONNX 文件。

#### 4.4 非关键数据延迟

`useAppStartupRuntime` 中的 `loadLibrary()`、`loadBootstrapStatus()`、`loadPlayerState()` 在
`libraryReady` 为 `true` 时触发。这些不阻塞首帧渲染，未改动。

### 验证

1. 执行 `pnpm tauri build --debug` 或 `pnpm tauri dev`
2. 启动后观察窗口：不应有白色→深色的跳变，窗口首次可见时应已有正确背景色
3. 确认 `LibrarySetup` 或 `AppLayout` 内容已渲染（而非空白）
4. 慢速机器（如 Intel Mac / VM）上尤其注意不应有可见的空白帧

### 工作量

~1-2 小时

---

## Phase 5: 设置 overlay 中剩余的 `window.prompt` 替换（已实现）

### 现状

`src/components/Settings/settings-overlay.library-actions.ts` 中仍有 2 处 `window.prompt`：

- `renameLibrary`：重命名库（输入新名称）
- `deleteLibrary`：删除库二次确认（输入库名确认）

与 Phase 1 中修复的 `Assign Singer` 和 `Create Playlist` 同根因：Tauri WebView 禁用 `window.prompt`，功能不可用。

### 实施结果

dialog 状态已放在 `SettingsLibrarySection.tsx` 的局部 `useState` 中管理（和
`Sidebar.tsx` 中 `showCreatePlaylist` 的模式一致），未改 `SettingsOverlayState`：

```typescript
// src/components/Settings/SettingsLibrarySection.tsx
const [renameDialog, setRenameDialog] = useState<{
  libraryId: string;
  currentName: string;
} | null>(null);
const [deleteConfirmDialog, setDeleteConfirmDialog] = useState<{
  libraryId: string;
  displayName: string;
} | null>(null);
```

- `actions.renameLibrary(libraryId, displayName)` 不再弹 prompt；按钮只打开 `InputDialog`
- `actions.deleteLibrary(libraryId, confirmationName)` 的 `window.confirm` 保留（Tauri 支持），但输入确认改为 `InputDialog`

JSX 中挂载 `InputDialog`，确认后回调 `handleRenameConfirm` / `handleDeleteConfirm`。

### 工作量

~1-2 小时

---

## Phase 6: 双击列表播放歌曲时的卡顿 + macOS 彩虹圈（已实现）

> Status: 方案 1–4 已实现；整轨预解码内存占用为桌面端可接受 tradeoff，流式播放列为后续 epic。

### 问题描述

在左侧曲库列表中**双击一首本地歌曲**后，"音频真正开始播放"之前会有一段明显卡顿，
macOS 鼠标指针变成彩虹旋转（beach ball）。即：双击 →（卡住一段时间，无任何反馈）→ 音频才出声。

### 调用链

```
SongListItem.handleDoubleClick           // src/components/Library/SongListItem.tsx:146
  └─ usePlayerStore.playSong(hash)        // src/stores/player-store.ts:197
       └─ workflow.playSong              // src/stores/playback-workflow.ts:56
            └─ await deps.play(songId)   // = api.play → invoke("play")
                 └─ #[tauri::command] play // src-tauri/src/commands/playback.rs:11 (async + spawn_blocking)
                      └─ services::playback::play  // src-tauri/src/services/playback.rs:95
```

### 根因 A：本地歌曲走"整轨同步预解码"路径，且全程无 loading 反馈

`services::playback::play`（`src-tauri/src/services/playback.rs:95`）对两类歌曲处理方式不同：

- **远程歌曲**（`needs_remote_download`）：先调用 `start_track_loading()` 立即返回一个
  `state: "loading"` 的快照，真正的下载 + 解码放到 `std::thread::spawn` 的后台线程里做。
  UI 立刻能显示 spinner，不会卡。
- **本地歌曲**：**同步**执行 `load_playback_source → load_song_audio → decode::decode_file`，
  而 `decode_file`（`src-tauri/src/audio/decode.rs:129-155`）会把**整首歌的全部 PCM
  采样一次性解码进 `Vec<f32>`** 之后才返回。**完全没有发出 loading 快照**。

后果：前端 `await deps.play(songId)` 在整轨解码完成前不会 resolve。对一首 4 分钟、
44.1kHz 立体声的歌，解码缓冲约 80MB，解码耗时可达 0.5–2s+；若是 Media+G ZIP
（`inspect_zip_for_media_g` 先把音频字节解压进内存再解码）或磁盘较慢，会更久。
这段时间 UI 没有任何"加载中"状态 → 用户感觉**整段卡死**。

> 注：`play` 命令本身是 `async` + `spawn_blocking`，解码在阻塞线程池上跑，
> 并不会直接阻塞 Tauri/Cocoa 主线程。所以"解码慢"只解释了"卡一段时间"，
> 还需根因 B 才能解释**彩虹圈**。

### 根因 B：曲库列表未虚拟化 + 每个列表项渲染开销 O(N)，播放开始瞬间触发全列表重渲染风暴

1. **列表未虚拟化**：`SongList.tsx` 直接 `displaySongs.map(...)` 渲染**全部**歌曲，
   曲库有几千首时就有几千个 `SongListItem` 同时挂载。
2. **每个 `SongListItem` 订阅整个 snapshot**：`SongListItem.tsx:52`
   `const snapshot = usePlayerStore((s) => s.snapshot)`。播放开始时
   `applySnapshot`（`player-store.ts:167`）替换了 snapshot 对象引用（`song_id`、
   `is_playing` 变化），zustand 默认按引用比较 → **每一个** `SongListItem` 都重渲染。
3. **每个列表项渲染做 O(N) 工作**：`SongListItem.tsx:73`
   `const selectedSongs = songs.filter(...)` 对整个曲库做一次过滤；
   随后又对所有 playlist 构建 `Map` 并逐个 `contextSongIds.filter`；
   还有 `.some/.every/.map` 和 `new Set(...)`。

于是一次"播放开始"会触发一轮 **O(N × (N + P)) ≈ O(N²)** 的同步重渲染：N 个列表项、
每项内部又遍历 N 首歌。几千首歌时就是数百万次操作压在**一个同步 JS 任务**里，
WebKit 渲染线程被卡住 → macOS 彩虹圈。

> 同样的风暴在**单击选中**（`selectSong` 改变 `selectedSongIds`）时也会发生，
> 这就是为什么连点选都感觉发涩。

### 综合时间线（用户感知）

```
双击
 → [根因A] 后端整轨同步解码，无 loading 快照……（数百毫秒~数秒，界面无反馈）
 → play() resolve，applySnapshot 替换 snapshot 引用
 → [根因B] 几千个 SongListItem 同步重渲染，每项 O(N) ……（彩虹圈尖峰）
 → 音频终于出声
```

### 加重因素

- `decode_file` 一次性分配整轨 PCM 缓冲（约 10MB/分钟），大块分配 + 解码 CPU 开销。
- **首次播放**还会触发 `ensure_output_thread`（`playback.rs:404`）初始化 cpal
  输出设备，带来一次性额外延迟。
- Media+G ZIP 路径需先解压音频字节进内存再解码，成本更高。

### 修复方案

| #   | 方案                                                                                                                                                                                                                                             | 解决的根因 | 预估   | 状态       |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------- | ------ | ---------- |
| 1   | **本地歌曲也先发 `loading` 快照再后台解码**：把本地路径改成和远程一致——先 `start_track_loading()` 立即返回，再用后台线程 `decode_then_start_track_if_latest`（已有该 helper + `request_id` 防竞态）。UI 立刻显示加载态，消除"无反馈卡死"。       | A          | 1-2 h  | **已实现** |
| 2   | **虚拟化曲库列表**：引入 `@tanstack/react-virtual`，只渲染可视区域行，把同时挂载的 `SongListItem` 从 N 降到常数。                                                                                                                                | B          | 2-3 h  | **已实现** |
| 3   | **把列表项里的 O(N) 计算移出去**：`songs.filter(...)`、playlist 成员关系、language 聚合等不应在每个列表项里算。改为父层 memo 化一次性算好，或只在"右键打开菜单时"惰性计算。                                                                      | B          | 1-2 h  | **已实现** |
| 4   | **收窄 snapshot 订阅**：`SongListItem` 不订阅整个 snapshot，改用返回原始值的 selector（如 `usePlayerStore((s) => s.snapshot?.song_id === song.hash && s.snapshot.is_playing)`），让 snapshot 引用变化时只有"当前/上一首"两项重渲染，而非整列表。 | B          | 30 min | **已实现** |

### 实施结果（方案 1–4 + 后台错误上报）

- **后端**（`src-tauri/src/services/playback.rs`）：`play()` 对所有歌曲统一先返回 `state: "loading"` 快照，再在 `play_track_background` 后台线程完成 decode + 换轨；失败时 `emit_playback_failure` 清除 loading 并发出 `playback-error`。
- **前端**（`src/components/Library/SongListItem.tsx`）：`isCurrentPlaying` / `isCurrentLoading` 原始值 selector；loading 时行首显示 `Loader2` spinner。
- **前端**（`src/components/Library/SongList.tsx`）：`@tanstack/react-virtual` 虚拟化，仅渲染可视行。
- **前端**（`song-list-item-context-menu-build.ts`）：右键菜单 O(N) 计算惰性化；`selectedSongIds.has(hash)` 收窄订阅。
- **前端**（`use-playback-runtime.ts`）：监听 `playback-error`，`notifyError` + 重试 `playSong`。
- **契约**（`phase-2-playback-contract.md`）：`play` 立即返回 loading；新增 `playback-error` 事件。

> 整轨预解码仍一次性分配 PCM 缓冲（约 10MB/分钟，四轨模式约 4×），这是当前 `PlaybackController` 的设计 tradeoff；改为流式 decode/play（含远程低带宽边下边播、多轨锁步同步）需要单独 epic，已拆出独立计划：[`2026-06-04-streaming-playback-optimization.md`](./2026-06-04-streaming-playback-optimization.md)。

### 验证

1. 准备一个 ≥2000 首歌的曲库（或临时放宽渲染数量）。
2. 双击一首本地长歌：应**立刻**看到加载/播放态变化（方案 1），且双击瞬间无彩虹圈（方案 4）。
3. 用 Web Inspector 的 Performance 录制双击过程，确认不再有单个 >100ms 的长任务来自 `SongListItem` 渲染。
4. `pnpm test`（含 `playback-workflow.test.ts`、`SongListItem.test.tsx`）无回归。

### 工作量

方案 4：~30 分钟；方案 1：~1-2 小时；方案 2/3：~半天。

---

## 实施结果汇总

| 阶段      | 结果                                                                                 |
| --------- | ------------------------------------------------------------------------------------ |
| Phase 4.1 | 已实现：`index.html` 内联深色背景                                                    |
| Phase 4.2 | 已实现：`visible: false` + `window_ready` IPC                                        |
| Phase 4.3 | 已实现：模型 verified manifest，减少启动同步大文件读取                               |
| Phase 1   | 已实现：移除显式 cursor pointer/default 声明                                         |
| Phase 5   | 已实现：Settings prompt 替换为 `InputDialog`                                         |
| Phase 2   | 待手动 audit：Tooltip/ContextMenu 边缘验证                                           |
| Phase 3   | 保持暂缓：resize 抗卡顿仅记录方案                                                    |
| Phase 6   | 已实现：方案 1–4 + 后台 decode 失败 `playback-error` 上报；整轨预解码为已知 tradeoff |

---

## 交付物

1. **代码变更**：
   - `index.html`：内联 `body { background-color: #121212; }`
   - `tauri.conf.json`：加入 `"visible": false, "center": true`
   - `src-tauri/src/commands/window_shell.rs`：新增 `window_ready` command
   - `src-tauri/src/lib.rs`：注册 `window_ready` command
   - 前端 `src/App.tsx` + `src/runtime/app-runtime.ts`：首屏渲染完成后调用 `window_ready`
   - `src-tauri/src/separator/bootstrap.rs`：模型 verified manifest，减少重复大文件校验
   - 前端显式 cursor pointer/default 移除
   - `SettingsLibrarySection.tsx`：`window.prompt` → `InputDialog`（局部 state）

2. **测试验证**：
   - `pnpm lint` → `pnpm build` → `pnpm test` → `cd src-tauri && cargo test -q`
   - 因修改 `tauri.conf.json`，最终 release gate 还需运行 `pnpm tauri build`
   - 手动启动测试：观察窗口首次可见时是否已有深色背景和实际内容

3. **文档**：
   - 本计划文档（Phase 3 技术方案仅供参考，不可直接使用）
   - `phase-6-model-bootstrap-contract.md` 更新模型 manifest 语义
   - ContextMenu/Tooltip 边缘 audit 仍需人工截图记录

---

## 风险与决策点

| 风险                                              | 影响                            | 缓解                                                                  |
| ------------------------------------------------- | ------------------------------- | --------------------------------------------------------------------- |
| `window_ready` 依赖前端渲染时机                   | 如果首帧 paint 慢，窗口显示延迟 | 4.1 内联 CSS 已消除白色闪烁，延迟是"全内容→可见"而非"白→内容"，可接受 |
| 模型 manifest 元数据变化                          | 首次迁移仍需完整校验一次        | 只有 manifest 匹配时跳过大文件读取；不匹配会重新校验并重写            |
| `_doAfterNextPresentationUpdate` 私有 API         | App Store 审核 + Tauri break    | 已放弃，改用跨平台 IPC 方案                                           |
| `NSWindow.setFrame` swizzle                       | Tauri 升级 break                | 暂缓执行，Phase 3 仅文档记录                                          |
| Phase 5 影响 `SettingsOverlay.controller.test.ts` | 测试 mock 需更新                | 已更新 action 测试，确认匹配文本才删除                                |

---

## 备注

- **不要**在 `tauri.conf.json` 中加 `hiddenTitle: true` 或 `transparent: true` —— `window_shell.m` 已通过 AppKit 手动配置 titlebar，config 选项会冲突
- Phase 5 中 `deleteLibrary` 的 `window.confirm` 保留不替换（Tauri WebView 支持 `confirm`）
