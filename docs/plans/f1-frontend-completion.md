# F1 前端补全实现计划

> **Status:** Completed · **Created:** 2026-05-14 · **Validated:** 2026-05-15
> **Scope:** 补全 F1（歌单 + 歌手轮唱）的前端缺失部分，修复 Issue #1–#3
> **Depends on:** 后端 IPC、SQLite 迁移、i18n 字符串均已就绪
> **Test report:** [`../testing/v0.9.0-test-report.md`](../testing/v0.9.0-test-report.md)

---

## 概览

四个工作流（Stream），按依赖顺序排列：

| Stream | 内容                                      | 涉及文件数 | 风险   |
| ------ | ----------------------------------------- | ---------- | ------ |
| **S1** | 全局禁用浏览器默认右键菜单                | 1          | Low    |
| **S2** | 歌曲右键菜单添加 "Add to Playlist" 子菜单 | 3          | Low    |
| **S3** | 歌单详情视图（替换曲库列表）              | 4          | Medium |
| **S4** | 歌手轮唱（队列面板增强）                  | 3+         | Medium |

S1 → S2 → S3 可串行实施。S4 独立于 S2/S3，但建议在 S3 之后进行（歌单详情视图提供歌手分配的视觉上下文）。

---

## S1 — 全局禁用浏览器默认右键菜单

**目标：** 消除 WebView 默认右键菜单（Paste / Reload / AutoFill），只保留应用自定义菜单。

### 修改文件

**`src/components/Layout/AppLayout.tsx`**

在根 `<div>` 上添加 `onContextMenu`：

```tsx
<div
  className="flex h-screen w-full flex-col overflow-hidden font-sans"
  onContextMenu={(e) => e.preventDefault()}
  // ... 其余属性不变
>
```

这不会影响 `SongListItem` 的自定义菜单——`SongListItem.handleContextMenu` 已经调用 `e.preventDefault()` 并设置自定义菜单状态，事件从内部组件冒泡到根节点之前已经被处理。

### 验证

- 在页面任意空白处右键 → 无菜单弹出
- 在歌曲行右键 → 仍显示自定义上下文菜单
- `pnpm test` 通过

---

## S2 — 歌曲右键菜单添加 "Add to Playlist" 子菜单

**目标：** 右键歌曲时，菜单中出现 "Add to Playlist…" 项，展开子菜单列出所有歌单。

### 交互设计

**单选（右键一首歌）：**

```
Play Now
Play Next
Add to Queue
Add to Playlist…  ▸
  ✓ My Favorites        ← indicator: "checked"（歌曲已在此歌单）
    Jazz Night
    Duet Practice
  ──────────────
  + New Playlist…       ← 创建歌单并直接加入
Extract Embedded Cover Art
...
```

**多选（选中多首歌）：**

```
Queue All Selected (3)
Add to Playlist…  ▸
  − My Favorites        ← indicator: "mixed"（部分歌曲在此歌单）
  ✓ Jazz Night          ← indicator: "checked"（全部在此歌单）
    Duet Practice
  ──────────────
  + New Playlist…
...
```

### 修改文件

#### 1. `src/components/Library/song-list-item-menu.ts`

**扩展参数接口 `BuildSongListContextMenuItemsArgs`：**

```ts
// 新增参数
playlists: Array<{ id: string; name: string }>;
songPlaylistMembership: Map<string, Set<string>>; // playlistId → set of songHashes in that playlist
onAddToPlaylist: (playlistId: string) => void;
onRemoveFromPlaylist: (playlistId: string) => void;
onCreatePlaylistAndAdd: () => void;
```

`songPlaylistMembership` 用于判断 indicator 状态：

- 如果所有选中歌曲都在某歌单 → `"checked"`
- 如果部分选中歌曲在某歌单 → `"mixed"`
- 如果都不在 → `null`

点击已在歌单中的项 → 调用 `onRemoveFromPlaylist`（取消关联）。
点击不在歌单中的项 → 调用 `onAddToPlaylist`（添加）。

**在 `buildSongListContextMenuItems` 函数中，** 在 "Add to Queue" 之后插入：

```ts
{
  label: t("playlist.addTo"),
  children: [
    ...playlists.map((p) => {
      const membership = computeIndicator(p.id, selectedSongIds, songPlaylistMembership);
      return {
        label: p.name,
        indicator: membership,
        onClick: () => {
          if (membership === "checked") {
            onRemoveFromPlaylist(p.id);
          } else {
            onAddToPlaylist(p.id);
          }
        },
      };
    }),
    {
      label: t("playlist.newPlaylist"),
      onClick: onCreatePlaylistAndAdd,
    },
  ],
},
```

#### 2. `src/components/Library/SongListItem.tsx`

在 `buildSongListContextMenuItems` 调用处添加新参数：

- 从 `usePlaylistStore` 读取 `playlists`
- 构建 `songPlaylistMembership`：按需查询或缓存各歌单的歌曲 hash 集合
- 为性能考虑，在右键打开时（`handleContextMenu` 内）一次性加载当前歌曲在各歌单的归属。可通过遍历 `playlists` 调用 `getPlaylistSongs` 实现，或在 store 中增加一个轻量查询方法。

**关于歌单归属查询的性能方案：**

为避免每次右键都调用 N 次 `getPlaylistSongs`，可在 `playlist-store` 中添加：

```ts
// playlist-store.ts 新增
playlistSongSets: Map<string, Set<string>>; // playlistId → Set<songHash>
loadPlaylistSongSets: () => Promise<void>;
```

在 `loadPlaylists` 时一并加载，或在歌曲加入/移除歌单后刷新。这样右键菜单读取时是同步操作，无延迟。

#### 3. `src/stores/playlist-store.ts`

新增 `playlistSongSets` 状态和加载方法：

```ts
playlistSongSets: new Map(),

loadPlaylistSongSets: async () => {
  const { playlists } = get();
  const sets = new Map<string, Set<string>>();
  for (const p of playlists) {
    const songs = await api.getPlaylistSongs(p.id);
    sets.set(p.id, new Set(songs.map((s) => s.song_hash)));
  }
  set({ playlistSongSets: sets });
},
```

在 `loadPlaylists`、`addSongsToPlaylist`、`removeSongsFromPlaylist` 完成后调用 `loadPlaylistSongSets`。

### 验证

- 右键歌曲 → "Add to Playlist…" 子菜单可展开
- 子菜单列出所有歌单，已包含的歌单显示 ✓
- 点击歌单名 → 歌曲被加入/移除 → toast 提示
- 点击 "+ New Playlist…" → 弹出命名 dialog → 创建歌单并将歌曲加入
- 多选歌曲 → 子菜单正确显示 checked / mixed 指示器
- `pnpm test` 通过

---

## S3 — 歌单详情视图（替换曲库列表）

**目标：** 点击侧边栏歌单后，SongList 区域切换为该歌单的歌曲列表。

### 交互设计

**进入歌单详情：**

- 点击侧边栏歌单行 → `setActivePlaylist(playlistId)`
- SongList 区域替换为歌单内容
- 歌单头部显示：返回箭头 + 歌单名（可编辑）

**退出歌单详情：**

- 点击返回箭头 → `setActivePlaylist(null)`
- 点击侧边栏 "All Tracks" 或 "Separated" → `setActivePlaylist(null)` + `setFilter(...)`
- 歌曲列表恢复为曲库视图

**歌单内歌曲的右键菜单变化：**

- 保留所有原有项（Play Now、Play Next 等）
- 新增 "Remove from Playlist" 项（调用 `removeSongsFromPlaylist`）
- "Add to Playlist…" 子菜单仍然可用（支持跨歌单操作）

### 修改文件

#### 1. `src/components/Library/SongList.tsx`

改造为支持两种模式：

```tsx
export function SongList() {
  const songs = useLibraryStore((s) => s.songs);
  const filter = useLibraryStore((s) => s.filter);
  const separationStatuses = useLibraryStore((s) => s.separationStatuses);
  const activePlaylistId = usePlaylistStore((s) => s.activePlaylistId);
  const [playlistSongs, setPlaylistSongs] = useState<Song[]>([]);

  useEffect(() => {
    if (activePlaylistId) {
      // 加载歌单歌曲，通过 hash 匹配 library songs 获取完整 Song 对象
      loadPlaylistSongsFromLibrary(activePlaylistId, songs).then(
        setPlaylistSongs,
      );
    }
  }, [activePlaylistId, songs]);

  const displaySongs = activePlaylistId
    ? playlistSongs
    : filter === "separated"
      ? songs.filter((s) => separationStatuses[s.hash]?.state === "completed")
      : songs;

  // ... 渲染逻辑不变，但传入 playlistId 给 SongListItem
}
```

#### 2. `src/components/Layout/Sidebar.tsx`

**添加歌单详情头部（在 SongList 之上）：**

当 `activePlaylistId` 非 null 时，在 "Song list" 区域上方显示：

```tsx
{
  activePlaylistId && (
    <div className="shrink-0 flex items-center gap-2 px-4 py-2 border-b ...">
      <button onClick={() => setActivePlaylist(null)}>
        <ArrowLeft size={14} />
      </button>
      <span className="text-[13px] font-medium truncate">
        {activePlaylist?.name}
      </span>
      {/* 可选：重命名、删除按钮 */}
    </div>
  );
}
```

**修改 filter tab 点击行为：** 点击 "All Tracks" / "Separated" 时同时清除 `activePlaylistId`：

```tsx
onClick={() => {
  setFilter("all");
  setActivePlaylist(null); // 退出歌单视图
}}
```

**修改 "Local Music" 标题：** 当 `activePlaylistId` 为 null 时显示 "Local Music"，否则不显示（因为歌单头部已经有名字了）。

#### 3. `src/components/Library/song-list-item-menu.ts`

新增参数：

```ts
activePlaylistId: string | null;
onRemoveFromActivePlaylist: () => void;
```

当 `activePlaylistId` 非 null 时，在菜单项中添加：

```ts
{
  label: t("playlist.removeFromPlaylist"),   // 需新增 i18n key
  onClick: onRemoveFromActivePlaylist,
}
```

#### 4. `src/locales/en.json` + `src/locales/zh-CN.json`

新增 key：

```json
"playlist.removeFromPlaylist": "Remove from Playlist"
"playlist.newPlaylist": "+ New Playlist…"
```

```json
"playlist.removeFromPlaylist": "从歌单中移除"
"playlist.newPlaylist": "+ 新建歌单…"
```

#### 5. `src/types/i18next.d.ts` — i18n 类型修复

当前 `Sidebar.tsx` 使用 `t("playlist.section")`、`t("playlist.create")` 等 key 时，LSP 报类型错误：

```
Type '"playlist.section"' is not assignable to type ...
```

**根因：** `src/types/i18next.d.ts` 通过 `import type en from "@/locales/en.json"` 声明 i18n key 类型。但 TypeScript 对 JSON 的类型推导可能因编辑器缓存而滞后，导致新增 key 未被识别。

**修复：** 在 S3 开始时，修改 `src/types/i18next.d.ts` 并立即还原（touch 操作即可），触发类型刷新：

```typescript
import type en from "@/locales/en.json";

// 显式确保 playlist.* 和 rotation.* namespace 的类型存在
// 如果 LSP 仍不识别，重启 TypeScript 语言服务器即可

declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "translation";
    resources: {
      translation: typeof en;
    };
  }
}
```

若修改文件后仍报错，在 VS Code 中执行 `Cmd+Shift+P` → `TypeScript: Restart TS Server`。此问题是编辑器缓存，不影响编译和运行。

### 验证

- 点击侧边栏歌单 → SongList 区域显示该歌单歌曲
- 歌单为空时显示空状态提示
- 返回箭头 → 恢复曲库视图
- 点击 "All Tracks" → 退出歌单并显示全部曲库
- 歌单内歌曲右键 → 有 "Remove from Playlist" 选项
- 移除后歌曲从歌单列表消失，曲库中仍存在
- `pnpm test` 通过
- `Sidebar.tsx` 中所有 `t("playlist.*")` 调用无 LSP 类型错误

---

## S4 — 歌手轮唱（队列面板增强）

**目标：** 在现有队列面板中集成歌手轮唱功能，作为队列的自然扩展。

### 交互设计

**队列面板结构（轮唱开启时）：**

```
┌──────────────────────────────┐
│ Up Next (5)       Clear All  │
├──────────────────────────────┤
│ 🎤 Rotation          [====] │  ← 开关 toggle
│                              │
│ Alice ✕ │ Bob ✕ │ Carol ✕ │  ← 歌手 tag 列表
│ [+ Add Singer]               │  ← 添加歌手（行内 input）
│ ▸ Alice                      │  ← 当前轮到的歌手加粗/高亮
├──────────────────────────────┤
│ ☰ Song A     Alice       ✕  │  ← 歌手名可点击编辑
│ ☰ Song B     Bob         ✕  │
│ ☰ Song C     Carol       ✕  │
│ ☰ Song D     Alice       ✕  │  ← round-robin 自动分配
│ ☰ Song E     Bob         ✕  │
└──────────────────────────────┘
```

**轮唱关闭时：**

- 歌手 tag 区域和每行的歌手列隐藏
- 队列行为与当前完全一致，无任何差异

### 新增文件

#### 1. `src/stores/rotation-store.ts`（新建）

```ts
interface RotationStoreState {
  active: boolean;
  singerNames: string[];
  currentIndex: number;
  mode: "round_robin" | "single";

  // 队列条目的歌手分配（内存态，不持久化到后端——
  // 后端 set_queue_entry_singer 仅用于歌单歌曲的歌手标注）
  queueSingers: Map<string, string | null>; // songHash → singer

  loadRotation: () => Promise<void>;
  toggleActive: () => Promise<void>;
  addSinger: (name: string) => Promise<void>;
  removeSinger: (name: string) => Promise<void>;
  advanceRotation: () => Promise<void>;
  assignSingerToQueueEntry: (songHash: string, singer: string | null) => void;
  getNextSinger: () => string | null;
}
```

**核心行为：**

- `loadRotation`：启动时从后端 `getRotationState` 加载
- `toggleActive`：切换 `active` 状态并持久化到后端 `setRotationState`
- `addSinger` / `removeSinger`：修改 `singerNames` 并持久化
- `advanceRotation`：调用后端 `advanceRotation`，推进 `currentIndex`
- `assignSingerToQueueEntry`：更新内存中的 `queueSingers` map
- `getNextSinger`：根据 `currentIndex` 和 `singerNames` 返回下一位歌手

**与 queue-store 的协作：**

- 当队列中添加新歌曲时（`addToQueue` / `playNext`），如果轮唱激活，自动调用 `getNextSinger()` 分配歌手并推进指针
- 实现方式：在 `SongListItem` 或 `song-list-item-menu` 的 `addToQueue` / `playNext` 回调中，增加歌手分配逻辑

### 修改文件

#### 2. `src/components/Player/QueuePanel.tsx`

**顶部新增轮唱控制区域（在 header 和列表之间）：**

```tsx
{
  /* Rotation controls — only when active or being set up */
}
<RotationControls />;
```

`RotationControls` 是一个内联子组件或独立文件，包含：

- Toggle 开关（`active` 状态绑定 `rotation-store`）
- 歌手 tag 列表（每个 tag 可删除）
- "+ Add Singer" 按钮（点击展开行内 input，带自动补全）
- 当前歌手高亮指示

**每个 `QueueItemCard` / `SortableQueueItem` 增加歌手字段：**

在 `QueueItemCard` 的 title / artist 旁边添加歌手名显示：

```tsx
// QueueItemCard 新增 prop
singer?: string | null;
onSingerClick?: () => void;

// 在 title/artist 列右侧显示
{rotationActive && (
  <button
    onClick={onSingerClick}
    className="shrink-0 text-[10px] text-[var(--color-accent)] ..."
  >
    {singer || t("rotation.noSinger")}
  </button>
)}
```

点击歌手名 → 弹出行内 input 或小型 dropdown（从 `singerNames` 列表选择或输入新名字）。

**歌手名编辑交互（行内 popover）：**

点击歌手名 → 在该位置弹出一个小 popover：

- 输入框（预填当前歌手名）
- 下拉列表显示已有歌手名（自动补全）
- 输入框 blur 或 Enter 确认
- 空输入 = 取消歌手分配

#### 3. `src/components/Player/RotationControls.tsx`（新建）

独立组件，放在 QueuePanel 的 header 和列表之间：

```tsx
export function RotationControls() {
  const {
    active,
    singerNames,
    currentIndex,
    toggleActive,
    addSinger,
    removeSinger,
  } = useRotationStore();
  const { t } = useTranslation();

  if (!active && singerNames.length === 0) {
    // 首次：只显示一个开关
    return (
      <div className="...">
        <label>{t("rotation.singer")}</label>
        <Toggle checked={active} onChange={toggleActive} />
      </div>
    );
  }

  return (
    <div className="border-b ... px-3 py-2 space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-[11px] font-semibold">
          {t("rotation.singer")}
        </span>
        <Toggle checked={active} onChange={toggleActive} />
      </div>

      {active && (
        <>
          <div className="flex flex-wrap gap-1">
            {singerNames.map((name, i) => (
              <SingerTag
                key={name}
                name={name}
                isCurrent={i === currentIndex}
                onRemove={() => removeSinger(name)}
              />
            ))}
            <AddSingerInput onAdd={addSinger} />
          </div>
        </>
      )}
    </div>
  );
}
```

### 自动分配逻辑

当歌曲通过 "Add to Queue" / "Play Next" 加入队列时：

```ts
// 在 SongListItem.tsx 的回调中
addToQueue: () => {
  useQueueStore.getState().addToQueue(song.hash);
  const rotation = useRotationStore.getState();
  if (rotation.active && rotation.singerNames.length > 0) {
    const singer = rotation.getNextSinger();
    rotation.assignSingerToQueueEntry(song.hash, singer);
    rotation.advanceRotation();
  }
},
```

### 验证

- 队列面板顶部显示 "Rotation" toggle
- 开启后显示歌手 tag 列表 + "Add Singer" 按钮
- 添加歌手 → tag 出现
- 新加入队列的歌曲自动分配下一位歌手
- 手动点击歌手名可修改
- 删除歌手 → tag 消失，该歌手分配的歌曲保持原歌手名不变
- 关闭 toggle → 歌手列和 tag 列表隐藏
- `pnpm test` 通过

---

## 实施顺序

```
S1 (全局右键禁用)
 ↓
S2 (Add to Playlist 子菜单)
 ↓
S3 (歌单详情视图)
 ↓
S4 (歌手轮唱)
 ↓
验证 + pnpm format → pnpm lint → pnpm build → pnpm test
```

每个 Stream 完成后独立验证，通过后再进入下一个。

---

## 文件变更总览

### 修改现有文件

| 文件                                            | Stream | 变更内容                                                               |
| ----------------------------------------------- | ------ | ---------------------------------------------------------------------- |
| `src/components/Layout/AppLayout.tsx`           | S1     | 根 div 添加 `onContextMenu`                                            |
| `src/components/Library/song-list-item-menu.ts` | S2, S3 | 新增 playlist 参数 + "Add to Playlist" 子菜单 + "Remove from Playlist" |
| `src/components/Library/SongListItem.tsx`       | S2, S3 | 传入 playlist 数据，歌单归属查询                                       |
| `src/stores/playlist-store.ts`                  | S2     | 新增 `playlistSongSets` 缓存                                           |
| `src/components/Library/SongList.tsx`           | S3     | 支持歌单模式（数据源切换）                                             |
| `src/components/Layout/Sidebar.tsx`             | S3     | 歌单详情头部 + filter 点击清除 activePlaylist                          |
| `src/components/Player/QueuePanel.tsx`          | S4     | 集成 RotationControls + 每行歌手字段                                   |
| `src/locales/en.json`                           | S3     | 新增 `playlist.removeFromPlaylist`、`playlist.newPlaylist`             |
| `src/locales/zh-CN.json`                        | S3     | 对应中文翻译                                                           |

### 新建文件

| 文件                                         | Stream | 内容                             |
| -------------------------------------------- | ------ | -------------------------------- |
| `src/stores/rotation-store.ts`               | S4     | 轮唱状态管理                     |
| `src/components/Player/RotationControls.tsx` | S4     | 轮唱控制 UI（toggle + 歌手列表） |

---

## 完成标准

1. Issue #1（右键菜单）：空白处右键无浏览器菜单
2. Issue #2（歌单添加歌曲）：右键菜单有 "Add to Playlist" 子菜单，功能正常
3. Issue #3（歌手轮唱）：队列面板集成轮唱 toggle + 歌手分配
4. 歌单详情视图：点击歌单 → 显示歌单歌曲 → 可移除 → 可返回
5. `pnpm format` → `pnpm lint` → `pnpm build` → `pnpm test` 全部通过
6. `docs/testing/v0.9.0-test-report.md` 更新为全部通过

## 验收结果

F1 前端补全已通过本地验收。验收中补充修复了歌单详情移除歌曲后当前列表不刷新的问题，并给歌单添加/移除操作补上成功 toast 与错误通知路径。

已运行验证：

- `pnpm format`
- `pnpm lint`
- `pnpm build`
- `pnpm test`
- `cargo fmt`
- `cd src-tauri && cargo test -q`
- `pnpm tauri build`
