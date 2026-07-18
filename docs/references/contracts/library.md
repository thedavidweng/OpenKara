# 资料库契约

字段或命令语义变更时，必须先更新此文档再改 UI。

## 接口

1. `import_songs(paths: Vec<String>, options?: ImportSongsOptions) -> ImportSongsResult`
2. `pick_import_paths(default_path?: String | null) -> Vec<String>`
3. `expand_import_paths(paths: Vec<String>) -> ExpandedImportPaths`
4. `get_library() -> Vec<Song>`
5. `search_library(query: String) -> Vec<Song>`
6. `set_songs_instrumental(song_ids: Vec<String>, instrumental: bool) -> Vec<Song>`
7. `extract_embedded_cover_art(song_ids: Vec<String>) -> ExtractEmbeddedCoverArtResult`
8. 本地元数据解析支持 MP3、FLAC、M4A
9. `songs` 表通过 `hash` 去重并执行 upsert

**F1 播放列表与歌手轮唱 (2026-05-13 新增):**

10. `list_playlists() -> Vec<Playlist>` — 返回所有播放列表及歌曲计数
11. `create_playlist(name: String) -> Playlist` — 创建新播放列表
12. `rename_playlist(playlist_id: String, name: String) -> ()` — 重命名播放列表
13. `delete_playlist(playlist_id: String) -> ()` — 删除播放列表及其关联歌曲
14. `add_songs_to_playlist(playlist_id: String, song_hashes: Vec<String>) -> ()` — 向播放列表添加歌曲
15. `remove_songs_from_playlist(playlist_id: String, song_hashes: Vec<String>) -> ()` — 从播放列表移除歌曲
16. `get_playlist_songs(playlist_id: String) -> Vec<PlaylistSong>` — 返回播放列表内歌曲
17. `set_rotation_state(rotation: RotationState) -> ()` — 设置轮唱状态
18. `get_rotation_state() -> RotationState` — 读取当前轮唱状态
19. `advance_rotation() -> RotationState` — 推进轮唱指针
20. `set_queue_entry_singer(playlist_id: String, song_hash: String, singer: Option<String>) -> ()` — 设置队列条目歌手
21. `playlists` 表和 `playlist_songs` 表通过 `ON DELETE CASCADE` 与 `songs` 表联动；删除歌曲时自动清理关联
22. `set_library_sort_mode(mode: LibrarySortMode) -> AppSettings` — 持久化资料库排序模式并返回更新后的全局设置
23. `get_cover_art(hash: String, size?: CoverArtSize) -> Option<Vec<u8>>` — 读取封面图原始字节或派生缩略图/预览图

## Inputs / outputs / required dependencies

### Command: `import_songs`

**Input**

```json
{
  "paths": ["/absolute/or/relative/audio/path.mp3"],
  "options": {
    "explicit_cdg_by_audio_path": {
      "/imports/song.flac": "/imports/song.cdg"
    },
    "skip_cdg_for_audio_paths": ["/imports/song.mp3"]
  }
}
```

**Output**

```json
{
  "imported": [
    {
      "hash": "sha256 hex string",
      "file_path": "/absolute/path/to/file.mp3",
      "instrumental": false,
      "title": "optional string",
      "artist": "optional string",
      "album": "optional string",
      "duration_ms": 123456,
      "cover_art": [137, 80, 78, 71],
      "imported_at": 1760000000
    }
  ],
  "failed": [
    {
      "path": "/bad/path.mp3",
      "error": {
        "code": "media_read_failed",
        "message": "failed to open audio file at /bad/path.mp3",
        "retryable": false,
        "fallback": "reimport_song"
      }
    }
  ]
}
```

**Semantics**

1. 单个路径失败不会中断整个批次，结果会落入 `failed`
2. 成功导入的项目会立即写入 SQLite，并返回写入后的 `Song`
3. `hash` 基于文件原始字节的 SHA-256，不基于路径
4. `file_path` 在返回前会被 canonicalize 为绝对路径
5. 若标签中没有标题，后端会回退到文件名 stem
6. 单个失败项的 `error` 已是结构化 `CommandError`，字段定义见 [errors.md](./errors.md)
7. 若用户只选择音频文件，而磁盘上存在同名 `.cdg` sidecar，后端会自动按 CD+G 成对导入
8. 若用户显式选择 `.cdg` 文件且前端已完成歧义消解，`options.explicit_cdg_by_audio_path` 会指定哪首音频应与该 `.cdg` 配对
9. `options.skip_cdg_for_audio_paths` 用于阻止同一 stem 的其他音频因同名 `.cdg` 被隐式配对

### Command: `expand_import_paths`

**Input**

```json
{
  "paths": ["/absolute/or/relative/folder/or/file"]
}
```

**Output**

```json
{
  "paths": ["/music/library/track-a.mp3", "/music/library/nested/track-b.flac"],
  "song_count": 2
}
```

**Semantics**

1. 该命令用于导入前预扫描，不写数据库，不复制媒体
2. 输入既可以是文件也可以是文件夹；文件夹会递归展开
3. 递归深度上限固定为 `3` 层，覆盖常见 `artist/album/song` 目录，同时避免大型目录或网络挂载把 UI 卡住
4. 输出 `paths` 只包含支持导入的媒体/歌词相关文件，且会去重并排序
5. `song_count` 表示确认弹窗中应展示的歌曲数量；当前按可导入文件数统计
6. 该命令的输出可直接作为后续 `import_songs` 的输入

### Command: `pick_import_paths`

**Input**

```json
{
  "default_path": "/Users/example/Music"
}
```

**Output**

```json
["/Users/example/Music/library", "/Users/example/Downloads/song.mp3"]
```

**Semantics**

1. 该命令负责打开导入选择器，本身不做扫描、不写数据库
2. macOS 上允许同一个原生面板同时选择文件和文件夹，且支持多选
3. 前端会把返回结果继续交给 `expand_import_paths` 做递归展开和数量确认
4. 非 macOS 当前不依赖此命令；前端保留直接文件选择回退

### Command: `get_library`

**Output:** `Vec<Song>`

**Semantics**

1. 排序为 `imported_at DESC, title COLLATE NOCASE ASC, hash ASC`
2. 当前不分页
3. 当前不做软删除过滤，因为还没有删除能力
4. 顶层命令失败时返回 `CommandError`，而不是自由文本字符串

### Command: `search_library`

**Input**

```json
{
  "query": "muse"
}
```

**Output:** `Vec<Song>`

**Semantics**

1. 大小写不敏感
2. 匹配范围：`title`、`artist`、`album`、`file_path`
3. 排序规则与 `get_library` 相同
4. 顶层命令失败时返回 `CommandError`

### Command: `extract_embedded_cover_art`

**Input**

```json
{
  "song_ids": ["sha256 song hash"]
}
```

**Output**

```json
{
  "updated_songs": [
    {
      "hash": "sha256 hex string",
      "file_path": "media/song.mp3",
      "title": "optional string",
      "artist": "optional string",
      "album": "optional string",
      "duration_ms": 123456,
      "cover_art": [137, 80, 78, 71],
      "imported_at": 1760000000
    }
  ],
  "failed": [
    {
      "song_id": "sha256 song hash",
      "error": {
        "code": "media_read_failed",
        "message": "song hash does not contain embedded cover art",
        "retryable": false,
        "fallback": "keep_current_state"
      }
    }
  ]
}
```

**Semantics**

1. 批量按顺序处理，单首失败不会中断其他歌曲
2. 成功项会覆盖 `songs.cover_art`，并返回更新后的完整 `Song`
3. 普通音频与 `paired` CDG 从磁盘音频文件读取封面；`ZIP+G` 从 ZIP 内音频字节读取封面
4. 若文件没有内嵌封面，当前数据库里的 `cover_art` 保持不变，并在 `failed` 中返回结构化错误
5. 顶层命令只在数据库不可用等整体失败时返回 `CommandError`

### Command: `get_cover_art`

**Input**

```json
{
  "hash": "sha256 song hash",
  "size": "thumb"
}
```

`size` 可选，取值 `"thumb"` | `"preview"` | `"original"`，默认 `"original"`。

**Output:** `Option<Vec<u8>>` — 请求尺寸的图片字节（`thumb`/`preview` 为 WebP，`original` 为原始格式）。无封面时返回 `null`。

**Semantics**

1. `async` 命令，磁盘解码在 `spawn_blocking` 线程执行，不阻塞 IPC 线程
2. `thumb` = 80×80 无损 WebP，`preview` = 256×256 无损 WebP，`original` = 数据库 `cover_art` BLOB 原始字节
3. 派生图文件名以封面字节的 SHA-256 为标识，存储在 library `artwork/` 目录下
4. 读取派生图时若文件缺失或损坏，会从原始 `cover_art` 惰性重新生成并写回路径（非致命）
5. 惰性修复仅在 `cover_art` BLOB 与生成时一致时才更新派生路径，避免并发替换封面后用旧派生路径覆盖新派生路径
6. 派生图生成失败时回退返回原始 `cover_art` 字节

### Remote Repository command semantics

远程资料库是 provider-hosted OpenKara repository，不是单纯的登录状态或本地数据库副本。合同术语如下：

| User model                     | Command surface                                                                       | Semantics                                                                                                                                               |
| ------------------------------ | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Refresh Repository             | `sync_active_remote_library`                                                          | 只把远端 `openkara.db` 和需要的文件刷新到本地 working copy；不发布本地修改。                                                                            |
| Publish Changes / Publish Song | `mirror_local_library_to_remote`, `publish_song_to_remote`, `publish_songs_to_remote` | 将本地 portable library 数据库和相关媒体文件写入远程资料库。发布前若 remote revision 已变化，必须停止并要求刷新后重试。                                 |
| Reauthorize Repository         | `reauthorize_remote_library`                                                          | 更新 OAuth token 或 WebDAV 凭据。OAuth provider 必须保持同一账号；WebDAV 用户名/密码可变，因为它们属于凭据。                                            |
| Relocate Repository            | `reauthorize_remote_library(..., allow_relocation=true)`                              | 用户重新授权时选中了不同远端位置。UI 必须先询问是否替换已保存的位置，并保留取消路径。后端只接受已有 OpenKara 资料库位置，不能把空目录初始化成新资料库。 |
| Disconnect Repository          | `remove_library`                                                                      | 只移除本地注册和本机凭据，不删除 provider-hosted 内容。                                                                                                 |
| Delete Repository              | `delete_library`                                                                      | 删除 provider-hosted 远程资料库内容和本地 working copy；UI 必须把它表达为永久删除远程资料库。                                                           |

新增命令：

1. `resolve_remote_library_candidate(session_id: String, display_name: String) -> RemoteLibraryCandidate`
   - 用当前授权会话和用户输入解析候选远端位置，不注册、不写配置。
   - WebDAV 用于在重新授权时比较新旧 repository location。
2. `reauthorize_remote_library(library_id: String, session_id: String, remote_root_locator: String, display_name: String, allow_relocation: bool) -> LibraryRegistrySnapshot`
   - 必须先验证目标位置已经包含 `.openkara-library` 和 `openkara.db`。
   - 验证成功后才写入新凭据、保存新的 remote root locator，并刷新本地 working copy。
   - 若位置变化且 `allow_relocation=false`，返回错误，等待 UI 进行用户确认。

### Command: `set_songs_instrumental`

**Input**

```json
{
  "song_ids": ["sha256 song hash"],
  "instrumental": true
}
```

**Output:** `Vec<Song>`

**Semantics**

1. 批量按请求顺序更新 `songs.instrumental`
2. 返回值包含每首更新后的完整 `Song`
3. `instrumental = true` 表示该歌曲被视为官方伴奏，不参与 AI 分离
4. `Media+G` 歌曲当前不会由前端发起该命令，但后端字段本身不额外限制素材类型
5. 若任一 `song_id` 不存在，命令返回顶层 `CommandError`

### Shared type: `Song`

| Field          | Type              | Notes                                          |
| -------------- | ----------------- | ---------------------------------------------- |
| `hash`         | `String`          | 全局稳定主键                                   |
| `file_path`    | `String`          | canonicalized 绝对路径                         |
| `instrumental` | `bool`            | 是否标记为官方伴奏；`true` 时不参与 AI 分离    |
| `title`        | `Option<String>`  | 可能为空                                       |
| `artist`       | `Option<String>`  | 可能为空                                       |
| `album`        | `Option<String>`  | 可能为空                                       |
| `duration_ms`  | `i64`             | 当前来自音频元数据                             |
| `cover_art`    | `Option<Vec<u8>>` | 原始图片字节，前端需自行转 data URL/object URL |
| `imported_at`  | `i64`             | Unix timestamp seconds                         |

### Shared type: `ImportFailure`

| Field   | Type           | Notes                                           |
| ------- | -------------- | ----------------------------------------------- |
| `path`  | `String`       | 原始输入路径                                    |
| `error` | `CommandError` | 结构化错误，字段定义见 [errors.md](./errors.md) |

### Shared type: `ImportSongsOptions`

| Field                        | Type                    | Notes                                          |
| ---------------------------- | ----------------------- | ---------------------------------------------- |
| `explicit_cdg_by_audio_path` | `Record<String,String>` | 指定某首音频应使用哪一个显式选择的 `.cdg` 文件 |
| `skip_cdg_for_audio_paths`   | `Vec<String>`           | 阻止这些音频在本次导入中被 `.cdg` 自动配对     |

### Shared type: `ExpandedImportPaths`

| Field        | Type          | Notes                            |
| ------------ | ------------- | -------------------------------- |
| `paths`      | `Vec<String>` | 递归展开、去重并排序后的导入路径 |
| `song_count` | `usize`       | 导入前确认弹窗使用的歌曲数量     |

### Shared type: `ExtractEmbeddedCoverArtResult`

| Field           | Type                                  | Notes                      |
| --------------- | ------------------------------------- | -------------------------- |
| `updated_songs` | `Vec<Song>`                           | 成功提取并写回封面的歌曲   |
| `failed`        | `Vec<ExtractEmbeddedCoverArtFailure>` | 逐首失败结果，允许部分成功 |

### Shared type: `ExtractEmbeddedCoverArtFailure`

| Field     | Type           | Notes                |
| --------- | -------------- | -------------------- |
| `song_id` | `String`       | 请求中的歌曲 hash    |
| `error`   | `CommandError` | 单首失败的结构化错误 |

### Command: `set_library_sort_mode`

**Input**

```json
{
  "mode": "recently_imported"
}
```

**Output**

返回更新后的 `AppSettings`（与 `set_lyrics_font_step` 相同的结构，详见 [lyrics.md](./lyrics.md)）。

**Semantics**

1. `mode` 是资料库排序模式，允许值固定为 `recently_imported`、`title_asc`、`artist_asc`
2. 该命令将排序模式持久化到应用 `config.json`，并返回更新后的全局 `AppSettings`
3. 排序模式仅影响前端 `SongList` 的显示顺序，不改变 `songs` 表的存储顺序
4. 非法值时命令返回 `CommandError`

### Shared type: `LibrarySortMode`

| Serialized value    | Meaning                  |
| ------------------- | ------------------------ |
| `recently_imported` | 按导入时间倒序（默认值） |
| `title_asc`         | 按标题升序               |
| `artist_asc`        | 按歌手升序               |

### Shared type: `CoverArtSize`

| Value        | Notes                                    |
| ------------ | ---------------------------------------- |
| `"thumb"`    | 80×80 无损 WebP 缩略图                   |
| `"preview"`  | 256×256 无损 WebP 预览图                 |
| `"original"` | 数据库 `cover_art` BLOB 原始字节（默认） |

### Required dependencies

1. Rust crate `lofty` 负责读标签和时长
2. Rust crate `rusqlite` 负责持久化
3. Rust crate `sha2` 负责生成稳定文件 hash
4. Tauri app setup 必须先完成 `AppState.database_path` 注入

## Verification commands

```bash
cd src-tauri
cargo test --test phase1_metadata --test phase1_cache --test phase1_import
cargo test
```

**Expected evidence**

1. `phase1_metadata`、`phase1_cache`、`phase1_import` 三个 integration tests 全部通过
2. `cache` 的 migration 单元测试通过
3. 无需运行 UI 也能验证导入、搜索、落库语义

## 资料库完整性审计 (Library Integrity Audit)

**I1 完整性审计与清理 (新增):**

22. `check_library_integrity() -> IntegrityReport` — 审计活动本地资料库的缺失/空引用文件和孤立管理文件
23. `remove_missing_library_entries(hashes: Vec<String>) -> IntegrityCleanupResult` — 在事务中重新验证并删除主媒体仍缺失/空的数据库条目

### Command: `check_library_integrity`

**Input**: 无参数

**Output**: `IntegrityReport`

**Semantics**

1. 在 `spawn_blocking` 中打开新连接，使用 `LEFT JOIN stems` 查询所有歌曲
2. 本地原始歌曲 (`audio_source_kind == "original"`) 计入 `checked_local_songs`；远程歌曲计入 `skipped_remote_songs`（远程歌曲的封面缩略图/预览图仍作为可选资产审计缺失/空）
3. 数据库相对路径必须使用正斜杠、无空段、`.`、`..`、绝对路径、盘符或反斜杠，并且必须指向该资产类型允许的 managed root 下的资产而不能仅为 `media` / `stems` 等根目录：主媒体只能在 `media/` 或 `media-g/`，CDG 只能在 `media-g/`，分轨只能在 `stems/`，封面衍生图只能在 `artwork/`；无效路径不会掩盖规范路径上的 orphan 或顶层 symlink，并按缺失资产报告。绝对、盘符、UNC 或 Windows 根相对路径在报告中统一脱敏为 `<invalid path>`，绝不泄露宿主机路径
4. 主媒体 (`file_path`) 缺失/非常规/无效路径 → `missing_primary_media`；零字节常规文件 → `empty_primary_media`
5. 可选资产 (CDG、分轨、封面缩略图/预览图) 缺失/空分别归入 `missing_optional_assets` / `empty_optional_assets`。封面衍生图还必须是严格命名的 `artwork/thumb_<64-lower-hex>_80.webp` 或 `artwork/preview_<64-lower-hex>_256.webp`、常规文件、真实 WebP，且尺寸精确为 80×80 / 256×256；错误格式或尺寸按缺失报告
6. 用 `symlink_metadata` 扫描 `media/`、`media-g/`、`stems/`、`artwork/`：不跟随任何符号链接。若某个顶层 managed root 本身是 symlink，则把相对根名（如 `media`）记入 `orphaned_managed_files` 并跳过该根；嵌套 symlink 以相对路径记为 orphan，且永不 `read_dir` 其目标
7. 仅 `artwork/` 的直接子项且完全匹配 writer 临时文件格式 `.{name}.{pid}.{counter}.tmp` 时，24 小时内排除；其他临时命名文件仍报告为孤立文件。不在有效引用集中的磁盘文件 → `orphaned_managed_files`（仅报告，不删除）
8. 所有向量按 `(song_hash, asset_type, path)` 排序去重；孤立路径按字典序排序
9. 相同文件系统/数据库状态必须跨运行字节级一致；审计不创建缺失的 `artwork/` 目录

### Command: `remove_missing_library_entries`

**Input**: `{ hashes: Vec<String> }`

**Output**: `IntegrityCleanupResult`

**Semantics**

1. 规范化输入：去空、排序、去重；空输入返回空结果
2. 开启 `BEGIN IMMEDIATE` 事务，逐首重新读取并验证
3. 仅当 `audio_source_kind == "original"` 且主媒体当前缺失/非常规/无效或零字节时才删除
4. 使用 `delete_song_rows_from_database` 原子删除（歌词、历史、分轨、播放列表 FK 联动）
5. 提交后尽力清理可选工作副本资产（没有任何存活歌曲引用时才删除 `media-g/` 下的 CDG sidecar、分轨目录，以及不再被任何歌曲引用的严格命名封面衍生图）；不删除非空主媒体文件，也不跟随 CDG/分轨/封面路径中的 symlink。提交后的清理失败只记录告警，数据库删除结果仍为权威，残留项将在下次审计中作为 orphan 报告。分轨清理只允许 `stems/` 下的直接子目录名，拒绝空值、`.`、`..`、分隔符、NUL、顶层 `stems/` symlink 和非目录目标；直接子 symlink 只删除链接本身，绝不递归其目标
6. 未知/远程/已恢复的 hash 计入 `skipped_song_hashes`
7. 数据库错误回滚整个批次；失败事务绝不触碰 playback 状态
8. 成功提交且 `deleted_song_hashes` 非空时，IPC 层尽力向 `PlaybackCoordinator` 发送内部 `InvalidateDeletedSongs`：清除匹配的 current/loading 轨道、仅在清除当前轨道时清空 CDG，并通过 `playback-position` 推送收敛后的 snapshot。协调器不可用或回复丢失仅记录告警，不改变已经提交的清理结果

### Shared type: `IntegrityReport`

| Field                     | Type                     | Notes                  |
| ------------------------- | ------------------------ | ---------------------- |
| `checked_local_songs`     | `usize`                  | 已检查的本地原始歌曲数 |
| `skipped_remote_songs`    | `usize`                  | 已跳过的远程歌曲数     |
| `missing_primary_media`   | `Vec<ManagedAssetIssue>` | 缺失的主媒体           |
| `empty_primary_media`     | `Vec<ManagedAssetIssue>` | 空的主媒体             |
| `missing_optional_assets` | `Vec<ManagedAssetIssue>` | 缺失的可选资产         |
| `empty_optional_assets`   | `Vec<ManagedAssetIssue>` | 空的可选资产           |
| `orphaned_managed_files`  | `Vec<String>`            | 孤立的管理文件路径     |

### Shared type: `ManagedAssetIssue`

| Field        | Type     | Notes                                                                                                                             |
| ------------ | -------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `song_hash`  | `String` | 歌曲 hash                                                                                                                         |
| `asset_type` | `String` | 固定值：`primary_media`/`cdg`/`stem_vocals`/`stem_accomp`/`stem_drums`/`stem_bass`/`stem_other`/`artwork_thumb`/`artwork_preview` |
| `path`       | `String` | 触发问题的数据库相对路径；相对非法值按原样回报，绝对/盘符/UNC/Windows 根相对值统一为 `<invalid path>`，合法值为规范路径             |

### Shared type: `IntegrityCleanupResult`

| Field                 | Type          | Notes              |
| --------------------- | ------------- | ------------------ |
| `deleted_song_hashes` | `Vec<String>` | 已删除的 hash 列表 |
| `skipped_song_hashes` | `Vec<String>` | 已跳过的 hash 列表 |
