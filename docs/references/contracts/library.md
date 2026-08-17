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
24. `get_import_candidate_details(paths: Vec<String>) -> Vec<ImportCandidateDetails>` — 预览导入候选文件的格式、比特率、文件大小和时长
25. `delete_songs(song_ids: Vec<String>) -> DeleteSongsResult` — 批量删除歌曲及其关联数据
26. `update_song_metadata(hash: String, title: Option<String>, artist: Option<String>) -> Song` — 更新单首歌曲的标题和歌手元数据
27. `set_songs_language(song_ids: Vec<String>, language: Option<String>) -> Vec<Song>` — 批量设置歌曲语言标签
28. `get_song_properties(song_id: String) -> SongProperties` — 读取单首歌曲的技术属性（格式、采样率、声道、比特率、文件大小、时长）
29. `create_library(path: String) -> LibraryRegistrySnapshot` — 在指定路径创建新本地资料库并设为活动
30. `open_library(path: String) -> LibraryRegistrySnapshot` — 打开指定路径的已有本地资料库并设为活动
31. `switch_library(library_id: String) -> LibraryRegistrySnapshot` — 切换活动资料库
32. `get_library_path() -> Option<String>` — 返回活动资料库的 canonicalized 根路径
33. `get_library_registry() -> LibraryRegistrySnapshot` — 返回所有已注册资料库及当前活动资料库 ID
34. `get_active_library() -> Option<RegisteredLibrary>` — 返回当前活动资料库的注册条目
35. `remove_library(library_id: String) -> LibraryRegistrySnapshot` — 移除资料库注册（本地不删文件，远程只移除凭据）
36. `rename_library(library_id: String, display_name: String) -> LibraryRegistrySnapshot` — 重命名资料库显示名
37. `delete_library(library_id: String) -> LibraryRegistrySnapshot` — 永久删除资料库（本地删除文件，远程删除 provider 内容）

**Online Sources / Streaming Import (issue #418):**

38. `streaming_track_identities` maps Streaming Track Identity `(source, remote_track_id)` to `songs.hash`. It does not replace `songs.hash` as the file primary key.
39. `playlist_origin_stamps` maps `(source, remote_playlist_id)` to one Playlist. A later Streaming Import of the same Streaming Playlist updates that Playlist and does not create a duplicate.
40. `get_reveal_targets(song_id: String) -> RevealTargets` — see [catalog.md](./catalog.md)
41. `reveal_in_folder(path: String) -> ()` — see [catalog.md](./catalog.md)
42. Library Decision for Import Conflict and CDG pairing share one prompt surface. Import Conflict actions are Keep, Replace, Apply to Remaining, and cancel. The prompt never includes the file hash.

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
5. `Song.artwork_thumb_path` 为 80×80 WebP 派生图的**绝对路径**（无封面或派生图尚未生成时为 `null`）。数据库列存的是 library 相对路径，命令在 IPC 边界改写为绝对路径，因为前端用 `convertFileSrc` 读取，该 API 只接受绝对路径
6. 返回这些路径的同时，命令会把 library `artwork/` 目录授予 asset protocol scope（非递归）。scope 是内存态且 library 根目录可迁移，所以授权必须发生在交出路径的时刻，而不是 library 激活时

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
5. `Song.artwork_thumb_path` 与 asset protocol 授权语义同 `get_library`

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

   曲库网格不再走这条路径读 `thumb`：它用 `Song.artwork_thumb_path` 经 asset protocol 直接读盘。`get_cover_art` 仍然是 `original` BLOB 的唯一来源，也是派生图缺失时的惰性修复入口——前端 `<img>` 加载失败时回落到这里

2. `thumb` = 80×80 无损 WebP，`preview` = 256×256 无损 WebP，`original` = 数据库 `cover_art` BLOB 原始字节
3. 派生图文件名以封面字节的 SHA-256 为标识，存储在 library `artwork/` 目录下
4. 读取派生图时若文件缺失或损坏，会从原始 `cover_art` 惰性重新生成并写回路径（非致命）
5. 惰性修复仅在 `cover_art` BLOB 与生成时一致时才更新派生路径，避免并发替换封面后用旧派生路径覆盖新派生路径
6. 派生图生成失败时回退返回原始 `cover_art` 字节

### Remote Repository command semantics

远程资料库是 provider-hosted OpenKara repository，不是单纯的登录状态或本地数据库副本。合同术语如下：

| User model                     | Command surface                                                                       | Semantics                                                                                                                                                                |
| ------------------------------ | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Refresh Repository             | `refresh_remote_repository`                                                           | 只把远端 `openkara.db` 和需要的文件刷新到本地 working copy；不发布本地修改。                                                                                             |
| Publish Changes / Publish Song | `mirror_local_library_to_remote`, `publish_song_to_remote`, `publish_songs_to_remote` | 将本地 portable library 数据库和相关媒体文件写入远程资料库。发布前若 remote revision 已变化，必须停止并要求刷新后重试。                                                  |
| Reauthorize Repository         | `reauthorize_remote_repository`                                                       | 更新 OAuth token 或 WebDAV 凭据。位置必须保持不变。OAuth provider 必须保持同一账号；WebDAV 用户名/密码可变，因为它们属于凭据。                                           |
| Relocate Repository            | `relocate_remote_repository`                                                          | 用户确认替换已保存的远端位置。UI 必须保留取消路径。后端只接受已有 OpenKara 资料库位置，不能把空目录初始化成新资料库。                                                    |
| Disconnect Repository          | `remove_library`                                                                      | 只移除本地注册和本机凭据，不删除 provider-hosted 内容。                                                                                                                  |
| Delete Repository              | `delete_library`                                                                      | 删除 provider-hosted 远程资料库内容和本地 working copy；UI 必须把它表达为永久删除远程资料库。                                                                            |
| Pre-Publish Conflict 出口      | `resolve_remote_conflict`                                                             | 仓库进入 `conflicted` 后的两条出路。`keep_local` 把本地 pending 变更 rebase 到胜出的远端 generation 之后重新发布；`use_remote` 丢弃 pending operation 并采用远端数据库。 |

新增命令：

0. `resolve_remote_conflict(resolution: "keep_local" | "use_remote") -> ()`
   - 只在活动远程资料库的 `local_state == conflicted` 时可用，否则返回错误。
   - 两条路径都先把胜出的远端数据库拉到 candidate 路径（不是 working copy）：在用户选择丢弃之前，本地 pending 变更仍是用户唯一的副本。
   - `keep_local` 仅在本地与远端改动触及**不相交**的歌曲、且仓库级设置未变时才自动 rebase；重叠时后端拒绝并要求显式选择，因为自动挑一方会静默丢失工作。
   - `use_remote` 把 operation 标记为 cancelled，用 candidate 覆盖 working copy 数据库，并把仓库状态推进到远端 generation 的 `Clean`。

1. `resolve_remote_library_candidate(session_id: String, display_name: String) -> RemoteLibraryCandidate`
   - 用当前授权会话和用户输入解析候选远端位置，不注册、不写配置。
   - WebDAV 用于在重新授权时比较新旧 repository location。
2. `reauthorize_remote_repository(library_id: String, session_id: String, remote_root_locator: String, display_name: String) -> LibraryRegistrySnapshot`
3. `relocate_remote_repository(library_id: String, session_id: String, remote_root_locator: String, display_name: String) -> LibraryRegistrySnapshot`
   - 必须先验证目标位置已经包含 `.openkara-library` 和 `openkara.db`。
   - 验证成功后才写入新凭据、保存新的 remote root locator，并刷新本地 working copy。
   - `reauthorize_remote_repository` 在位置变化时返回错误。
   - `relocate_remote_repository` 只接受已存在的 OpenKara 资料库位置。

### Command: `begin_remote_auth`

**Input**

```json
{
  "provider": "google_drive",
  "payload": null
}
```

`provider` is one of `google_drive`, `dropbox`, `webdav`. `payload` is an optional provider-specific JSON object (e.g. WebDAV server URL and credentials).

**Output:** `RemoteAuthStart`

```json
{
  "session_id": "session-uuid",
  "provider": "google_drive",
  "authorization_url": "https://...",
  "expires_at_ms": 1760000000000
}
```

**Semantics**

1. Starts an OAuth or WebDAV authentication session for the given provider
2. For OAuth providers, returns an `authorization_url` the frontend opens in the system browser
3. For WebDAV, the `payload` carries the server URL and credentials; no browser redirect is needed
4. The session ID is used by `poll_remote_auth` and `cancel_remote_auth`

### Command: `poll_remote_auth`

**Input**

```json
{
  "session_id": "session-uuid"
}
```

**Output:** `RemoteAuthStatus`

```json
{
  "session_id": "session-uuid",
  "provider": "google_drive",
  "state": "ready",
  "remote_root_locator": "google_drive://account-id/path",
  "display_name": "My Drive",
  "error": null
}
```

**Semantics**

1. `state` is one of `pending`, `ready`, `failed`
2. The frontend polls this command until `state` is `ready` or `failed`
3. When `state = ready`, `remote_root_locator` and `display_name` are set
4. When `state = failed`, `error` contains a `CommandError`

### Command: `cancel_remote_auth`

**Input**

```json
{
  "session_id": "session-uuid"
}
```

**Output:** `()`

**Semantics**

1. Cancels an in-progress authentication session
2. After cancellation, `poll_remote_auth` for the same session ID returns `failed`

### Command: `open_external_url`

**Input**

```json
{
  "url": "https://..."
}
```

**Output:** `()`

**Semantics**

1. Opens the given URL in the system default browser
2. Used for OAuth authorization URLs and help links

### Command: `list_remote_library_roots`

**Input**

```json
{
  "session_id": "session-uuid"
}
```

**Output:** `Vec<RemoteLibraryCandidate>`

**Semantics**

1. Lists existing OpenKara library directories found in the authenticated provider account
2. Each candidate includes the provider, remote root locator, display name, and account ID
3. The frontend shows these as options when the user chooses which remote library to register

### Command: `create_remote_library`

**Input**

```json
{
  "session_id": "session-uuid",
  "display_name": "My New Library"
}
```

**Output:** `RemoteLibraryCandidate`

**Semantics**

1. Creates a new OpenKara library directory in the authenticated provider account
2. Does not register the library in app config; the frontend calls `register_remote_library` after the user confirms
3. Returns the candidate with the new remote root locator

### Command: `register_remote_library`

**Input**

```json
{
  "session_id": "session-uuid",
  "remote_root_locator": "google_drive://account-id/path",
  "display_name": "My Remote Library"
}
```

**Output:** `LibraryRegistrySnapshot`

**Semantics**

1. Registers the remote library in app config using the authenticated session
2. Downloads the remote `openkara.db` and media files to the local working copy
3. Sets the new library as the active library
4. If the remote location does not contain a valid OpenKara library, the command returns `CommandError`

### Command: `get_all_upload_statuses`

**Input**: none

**Output:** `Vec<UploadStatusSnapshot>`

```json
[
  {
    "song_id": "sha256 song hash",
    "state": "running",
    "percent": 42,
    "remote_library_id": "library-uuid",
    "detail": null,
    "error": null
  }
]
```

**Semantics**

1. Returns the current upload status for all songs that have an active or recent upload
2. `state` is one of `idle`, `running`, `completed`, `failed`
3. The frontend uses this to show upload progress indicators in the library

### Shared type: `RemoteAuthStart`

| Field               | Type                    | Notes                                           |
| ------------------- | ----------------------- | ----------------------------------------------- |
| `session_id`        | `String`                | Session ID for polling and cancellation         |
| `provider`          | `RemoteLibraryProvider` | The provider being authenticated                |
| `authorization_url` | `Option<String>`        | OAuth URL to open in browser; `null` for WebDAV |
| `expires_at_ms`     | `Option<i64>`           | Session expiry wall-clock ms                    |

### Shared type: `RemoteAuthStatus`

| Field                 | Type                    | Notes                           |
| --------------------- | ----------------------- | ------------------------------- |
| `session_id`          | `String`                | Session ID                      |
| `provider`            | `RemoteLibraryProvider` | The provider                    |
| `state`               | `RemoteAuthState`       | `pending`, `ready`, or `failed` |
| `remote_root_locator` | `Option<String>`        | Set when `state = ready`        |
| `display_name`        | `Option<String>`        | Set when `state = ready`        |
| `error`               | `Option<CommandError>`  | Set when `state = failed`       |

### Shared type: `RemoteLibraryCandidate`

| Field                 | Type                    | Notes                               |
| --------------------- | ----------------------- | ----------------------------------- |
| `provider`            | `RemoteLibraryProvider` | The provider                        |
| `remote_root_locator` | `String`                | Provider-specific root locator      |
| `remote_path_display` | `String`                | Human-readable path in the provider |
| `display_name`        | `String`                | Library display name                |
| `account_id`          | `String`                | Provider account ID                 |

### Shared type: `UploadStatusSnapshot`

| Field               | Type                   | Notes                                       |
| ------------------- | ---------------------- | ------------------------------------------- |
| `song_id`           | `String`               | The song hash                               |
| `state`             | `UploadState`          | `idle`, `running`, `completed`, or `failed` |
| `percent`           | `u8`                   | Upload progress (0–100)                     |
| `remote_library_id` | `Option<String>`       | Target remote library ID                    |
| `detail`            | `Option<String>`       | Diagnostic detail string                    |
| `error`             | `Option<CommandError>` | Set when `state = failed`                   |

### Shared enum: `RemoteLibraryProvider`

| Serialized value | Meaning      |
| ---------------- | ------------ |
| `google_drive`   | Google Drive |
| `dropbox`        | Dropbox      |
| `web_dav`        | WebDAV       |

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

### Command: `get_import_candidate_details`

**Input**

```json
{
  "paths": ["/absolute/or/relative/audio/path.mp3"]
}
```

**Output:** `Vec<ImportCandidateDetails>`

```json
[
  {
    "path": "/music/track.mp3",
    "format": "mp3",
    "bit_rate": 320000,
    "file_size": 5242880,
    "duration_ms": 180000
  }
]
```

**Semantics**

1. Reads each path and probes its audio format, bit rate, file size, and duration
2. Does not write to the database or copy media
3. The frontend uses this to show file details in the import confirmation dialog
4. If a file cannot be read or probed, the command returns `CommandError` with `code = media_read_failed`

### Command: `delete_songs`

**Input**

```json
{
  "song_ids": ["sha256 song hash"]
}
```

**Output:** `DeleteSongsResult`

```json
{
  "deleted_song_ids": ["sha256 song hash"],
  "failed": [
    {
      "song_id": "sha256 song hash",
      "error": {
        "code": "media_read_failed",
        "message": "failed to delete song files",
        "retryable": false,
        "fallback": "keep_current_state"
      }
    }
  ]
}
```

**Semantics**

1. Deletes each song's database rows (lyrics, history, stems, playlist FKs) and managed media files
2. A single song failure does not abort the batch; failures fall into `failed`
3. If the currently playing song is among the deleted songs, the backend clears the playback track and CDG state
4. Returns the list of successfully deleted song IDs and per-song failures

### Command: `update_song_metadata`

**Input**

```json
{
  "hash": "sha256 song hash",
  "title": "optional new title",
  "artist": "optional new artist"
}
```

**Output:** `Song` — the updated song with absolute thumbnail path.

**Semantics**

1. Updates `songs.title` and `songs.artist` for the given hash
2. `null` for `title` or `artist` clears the field; the field name is retained for IPC stability
3. The command publishes the updated song to the active remote library if one is connected
4. If the song does not exist, the command returns `CommandError`

### Command: `set_songs_language`

**Input**

```json
{
  "song_ids": ["sha256 song hash"],
  "language": "en"
}
```

Pass `null` for `language` to clear the language tag.

**Output:** `Vec<Song>`

**Semantics**

1. Batch-updates `songs.language` for each requested song
2. Returns the updated songs with absolute thumbnail paths
3. The command mirrors the change to the active remote library if one is connected
4. If any `song_id` does not exist, the command returns `CommandError`

### Command: `get_song_properties`

**Input**

```json
{
  "song_id": "sha256 song hash"
}
```

**Output:** `SongProperties`

```json
{
  "format": "mp3",
  "sample_rate": 44100,
  "channels": 2,
  "bit_rate": 320000,
  "file_size": 5242880,
  "duration_ms": 180000,
  "hash": "sha256 song hash"
}
```

**Semantics**

1. Probes the song file on disk for technical properties (format, sample rate, channels, bit rate, file size, duration)
2. For remote songs, the command ensures the file is cached before probing
3. If the song does not exist, the command returns `CommandError`

### Command: `create_library`

**Input**

```json
{
  "path": "/path/to/new/library"
}
```

**Output:** `LibraryRegistrySnapshot`

**Semantics**

1. Creates a new library directory structure at the given path
2. Initializes the library SQLite database
3. Registers the library in app config and sets it as the active library
4. If the path already contains a library, the command returns `CommandError`

### Command: `open_library`

**Input**

```json
{
  "path": "/path/to/existing/library"
}
```

**Output:** `LibraryRegistrySnapshot`

**Semantics**

1. Opens an existing library directory at the given path
2. Initializes the library SQLite database if needed
3. Registers the library in app config and sets it as the active library
4. If the path does not contain a valid library, the command returns `CommandError`

### Command: `switch_library`

**Input**

```json
{
  "library_id": "library-uuid"
}
```

**Output:** `LibraryRegistrySnapshot`

**Semantics**

1. Switches the active library to the one identified by `library_id`
2. Clears all library-scoped runtime state (playback, CDG, remote upload statuses) before activating the new library
3. If the library ID is not found, the command returns `CommandError`

### Command: `get_library_path`

**Input**: none

**Output:** `Option<String>` — the canonicalized absolute path of the active library root, or `null` when no library is active.

### Command: `get_library_registry`

**Input**: none

**Output:** `LibraryRegistrySnapshot`

```json
{
  "active_library_id": "library-uuid",
  "libraries": [
    {
      "kind": "local",
      "id": "library-uuid",
      "display_name": "My Library",
      "root_path": "/path/to/library"
    }
  ]
}
```

**Semantics**

1. Reads the app config and returns all registered libraries plus the active library ID
2. Does not open any database or touch disk

### Command: `get_active_library`

**Input**: none

**Output:** `Option<RegisteredLibrary>` — the active library entry, or `null` when no library is active.

### Command: `remove_library`

**Input**

```json
{
  "library_id": "library-uuid"
}
```

**Output:** `LibraryRegistrySnapshot`

**Semantics**

1. Removes the library from app config and removes its stored credentials
2. For local libraries, does not delete files on disk
3. For remote libraries, removes only local credentials and registration; provider-hosted content is not deleted
4. If the removed library was active, the backend activates the first remaining library or clears all library-scoped state when no library remains
5. If the library ID is not found, the command returns `CommandError`

### Command: `rename_library`

**Input**

```json
{
  "library_id": "library-uuid",
  "display_name": "New Name"
}
```

**Output:** `LibraryRegistrySnapshot`

**Semantics**

1. Updates the display name of the registered library in app config
2. Does not rename the library directory on disk
3. If the library ID is not found, the command returns `CommandError`

### Command: `delete_library`

**Input**

```json
{
  "library_id": "library-uuid"
}
```

**Output:** `LibraryRegistrySnapshot`

**Semantics**

1. Permanently deletes the library data, then calls `remove_library`
2. For local libraries, deletes the entire library directory from disk
3. For remote libraries, deletes the provider-hosted content and the local working copy
4. The UI must present this as a permanent destructive action
5. If the library ID is not found, the command returns `CommandError`

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

### Shared type: `ImportCandidateDetails`

| Field         | Type          | Notes                             |
| ------------- | ------------- | --------------------------------- |
| `path`        | `String`      | The probed file path              |
| `format`      | `String`      | Audio format (e.g. `mp3`, `flac`) |
| `bit_rate`    | `Option<u32>` | Bit rate in bits per second       |
| `file_size`   | `u64`         | File size in bytes                |
| `duration_ms` | `Option<i64>` | Duration in milliseconds          |

### Shared type: `DeleteSongsResult`

| Field              | Type                      | Notes                                       |
| ------------------ | ------------------------- | ------------------------------------------- |
| `deleted_song_ids` | `Vec<String>`             | Successfully deleted song hashes            |
| `failed`           | `Vec<DeleteSongsFailure>` | Per-song failures, allowing partial success |

### Shared type: `DeleteSongsFailure`

| Field     | Type           | Notes              |
| --------- | -------------- | ------------------ |
| `song_id` | `String`       | The song hash      |
| `error`   | `CommandError` | The failure reason |

### Shared type: `SongProperties`

| Field         | Type          | Notes                             |
| ------------- | ------------- | --------------------------------- |
| `format`      | `String`      | Audio format (e.g. `mp3`, `flac`) |
| `sample_rate` | `Option<u32>` | Sample rate in Hz                 |
| `channels`    | `Option<u16>` | Number of audio channels          |
| `bit_rate`    | `Option<u32>` | Bit rate in bits per second       |
| `file_size`   | `u64`         | File size in bytes                |
| `duration_ms` | `i64`         | Duration in milliseconds          |
| `hash`        | `String`      | The song hash                     |

### Shared type: `LibraryRegistrySnapshot`

| Field               | Type                     | Notes                        |
| ------------------- | ------------------------ | ---------------------------- |
| `active_library_id` | `Option<String>`         | Active library ID, or `null` |
| `libraries`         | `Vec<RegisteredLibrary>` | All registered libraries     |

### Shared type: `RegisteredLibrary`

Tagged union with `kind` discriminator.

| Variant  | Fields                                                                                                                                                    | Notes                            |
| -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| `local`  | `id`, `display_name`, `root_path`                                                                                                                         | A local on-disk library          |
| `remote` | `id`, `display_name`, `provider`, `account_id`, `remote_root_locator`, `remote_path_display`, `connection_config?`, `cached_db_path?`, `remote_revision?` | A provider-hosted remote library |

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

## 资料库完整性审计 (Library Integrity Audit)

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
| `path`       | `String` | 触发问题的数据库相对路径；相对非法值按原样回报，绝对/盘符/UNC/Windows 根相对值统一为 `<invalid path>`，合法值为规范路径           |

### Shared type: `IntegrityCleanupResult`

| Field                 | Type          | Notes              |
| --------------------- | ------------- | ------------------ |
| `deleted_song_hashes` | `Vec<String>` | 已删除的 hash 列表 |
| `skipped_song_hashes` | `Vec<String>` | 已跳过的 hash 列表 |
