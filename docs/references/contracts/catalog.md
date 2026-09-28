# Catalog Contract

Field or command semantic changes must update this document before changing UI
code. Domain terms come from `CONTEXT.md`. Decisions: ADR 0031–0035.

## Types

`OnlineSourceId`: `"youtube"` | `"netease"`

`OnlineSourceKind`: `"video"` | `"streaming"`

`OnlineSourceCapabilities`

```json
{
  "sign_in": false,
  "browse": false,
  "import": false,
  "resolve_video": false
}
```

`OnlineSourceSnapshot`

```json
{
  "id": "youtube",
  "kind": "video",
  "enabled": false,
  "capabilities": {
    "sign_in": false,
    "browse": false,
    "import": false,
    "resolve_video": false
  }
}
```

`StreamingSessionSnapshot`

```json
{
  "source_id": "netease",
  "signed_in": false,
  "display_name": null,
  "expired": false
}
```

`ImportRefusalReason`: `"no_play_rights"` | `"trial_clip"` | `"empty_url"`

`StreamingTrack`

```json
{
  "source_id": "netease",
  "remote_track_id": "123",
  "title": "Title",
  "artist": "Artist",
  "album": "Album",
  "duration_ms": 180000,
  "refusal": null
}
```

`ImportRefusal`

```json
{
  "reason": "trial_clip",
  "title": "Title",
  "artist": "Artist"
}
```

`StreamingPlaylistSummary`

```json
{
  "remote_id": "pl-1",
  "name": "Night set",
  "track_count": 12
}
```

`StreamingPlaylistDetail`

```json
{
  "remote_id": "pl-1",
  "name": "Night set",
  "tracks": []
}
```

`LibraryDecisionMeta`

```json
{
  "title": "Title",
  "artist": "Artist",
  "album": "Album",
  "format": "MP3",
  "bit_rate_bps": 320,
  "duration_ms": 180000,
  "file_size_bytes": 4200000
}
```

`ImportConflictPrompt`

```json
{
  "source_id": "netease",
  "remote_track_id": "123",
  "library": {},
  "incoming": {}
}
```

`LibraryDecisionAction`: `"keep"` | `"replace"` | `"apply_keep"` | `"apply_replace"` | `"cancel"`

`StreamingImportFailureReason`: `"refusal"` | `"cancelled"` | `"import_failed"`

`StreamingImportFailure`

```json
{
  "remote_track_id": "123",
  "title": "Title",
  "artist": "Artist",
  "reason": "refusal",
  "refusal": {
    "reason": "trial_clip",
    "title": "Title",
    "artist": "Artist"
  }
}
```

`StreamingImportProgress`

```json
{
  "status": "completed",
  "imported_song_ids": [],
  "failed": [],
  "playlist_id": null,
  "conflict": null
}
```

`StreamingImportProgress.status`: `"awaiting_decision"` | `"completed"`

`VideoQueueItem`

```json
{
  "id": "yt:dQw4w9WgXcQ",
  "title": "Title",
  "channel": "Channel",
  "duration_ms": 180000,
  "thumbnail_url": null,
  "watch_url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
}
```

`VideoUnavailableReason`: `"invalid_url"` | `"age_restricted"` | `"private"` | `"unlisted"` | `"unavailable"`

`RevealTarget`

```json
{
  "available": true,
  "path": "/absolute/path"
}
```

`RevealTargets`

```json
{
  "song_file": { "available": true, "path": "/abs/media/hash.mp3" },
  "stems": { "available": false, "path": null }
}
```

## Commands

1. `list_online_sources() -> Vec<OnlineSourceSnapshot>`
2. `set_online_source_enabled(source_id: OnlineSourceId, enabled: bool) -> AppSettings`
3. `get_streaming_session(source_id: OnlineSourceId) -> StreamingSessionSnapshot`
4. `open_streaming_sign_in(source_id: OnlineSourceId) -> ()`
5. `sign_out_streaming_source(source_id: OnlineSourceId) -> StreamingSessionSnapshot`
6. `list_streaming_liked_tracks(source_id: OnlineSourceId) -> Vec<StreamingTrack>`
7. `list_streaming_playlists(source_id: OnlineSourceId) -> Vec<StreamingPlaylistSummary>`
8. `get_streaming_playlist(source_id: OnlineSourceId, remote_playlist_id: String) -> StreamingPlaylistDetail`
9. `search_streaming_source(source_id: OnlineSourceId, query: String) -> Vec<StreamingTrack>`
10. `start_streaming_import(source_id: OnlineSourceId, remote_track_ids: Vec<String>, remote_playlist_id: Option<String>) -> StreamingImportProgress`
11. `continue_streaming_import(action: LibraryDecisionAction) -> StreamingImportProgress`
12. `resolve_video_source_url(source_id: OnlineSourceId, url: String) -> Vec<VideoQueueItem>`
13. `get_reveal_targets(song_id: String) -> RevealTargets`
14. `reveal_in_folder(path: String) -> ()`

## Events

- `netease-sign-in-complete` targets `main` with a verified `StreamingSessionSnapshot`.
- `netease-sign-in-error` targets `main` with a `CommandError` and no credentials.

## Semantics

1. The registry always returns YouTube then NetEase, in that order.
2. Both sources default to `enabled: false`.
3. `set_online_source_enabled` persists only that source. It does not sign out
   and it does not clear Streaming Credentials.
4. An unknown `source_id` returns `CommandError` with `code: internal`.
5. A disabled source rejects browse, sign-in, import, resolve, and YouTube
   resolve commands with `code: online_source_disabled`.
6. `AppSettings.youtube_source_enabled` and `AppSettings.netease_source_enabled`
   match the registry flags after every successful set or `get_settings`.
7. When a source is off, its `capabilities` are all `false`. When YouTube is on,
   only `resolve_video` is true. When NetEase is on, `sign_in`, `browse`, and
   `import` are true.
8. Streaming Credentials are `MUSIC_U` and `__csrf` only. They live under the
   keychain service `org.openkara.streaming-source`, separate from Repository
   Credentials. OpenKara never receives a password or SMS code. The official
   `music.163.com` page runs as the top-level document in a dedicated incognito
   window without Tauri capabilities. While it is open, the host checks only
   the two session cookies. When both appear, it verifies the account, stores
   the credentials and display name, closes the window, and emits the result.
   Failed verification stores nothing, closes the window, and emits an error.
9. Sign-out clears Streaming Credentials. Turning NetEase off does not.
10. `get_streaming_session` verifies stored credentials against the account
    operation. An expired session returns `signed_in: false` and `expired: true`.
    A code 301 from any NetEase operation returns
    `code: streaming_session_expired` and returns the UI to sign-in.
11. NetEase operations run through the pinned Enhanced API sidecar. The sidecar
    owns endpoint paths, encryption, and China Client Address headers. It has
    line-delimited JSON over standard input/output and no listening socket.
    A missing sidecar reports an integration error. Other provider rejections,
    including code 8860, remain `code: streaming_auth_failed` with the
    provider code; they do not trigger an alternative identity or source.
12. Resolve of a Streaming Track returns an importable temp file or an
    Import Refusal. A no-play-rights song, trial clip, or empty stream URL is an
    Import Refusal. A pricing flag alone does not refuse an available full URL. An Import Refusal does not download and does not fetch a
    replacement from another platform.
13. Streaming Import writes a temp file, then calls the existing song import.
    A successful import is a normal library song.
14. A whole-list import creates or updates one Playlist via a Playlist Origin
    Stamp `(source, remote playlist id)`. A later import of the same Streaming
    Playlist adds only missing tracks. Remote deletions do not remove local
    Playlist songs.
15. Identity is Streaming Track Identity `(source, remote track id)`, not title
    or artist. Same file hash is a silent no-op. Same identity with different
    bytes is an Import Conflict.
16. An Import Conflict is a Library Decision. The prompt shows title, artist,
    album, format, bit rate, duration, and size. It never shows the file hash.
    Keep Library Song leaves the file. Replace Library Song swaps audio, keeps
    lyrics and playlist membership, and drops stale stems and waveform cache.
    Apply to Remaining covers only later Import Conflicts in this import.
    Closing the dialog (`cancel`) stops the rest of the batch.
17. Selected Import Refusals and cancelled items appear on the failure list.
18. A public YouTube watch link becomes one queue item. A public playlist link
    expands into ordered queue items. Queue ids use the `yt:` prefix. Those
    ids are never written to `playlist_songs`.
19. Age-restricted, private, or unlisted YouTube items fail with
    `code: video_source_unavailable` and a typed `VideoUnavailableReason`.
    Resolve does not call YouTube `/player` stream URLs.
20. `get_reveal_targets` resolves the song file and stem folder to absolute
    paths. A missing target has `available: false`. `reveal_in_folder` opens
    the system file manager on an existing path.
21. There is no UNM crate, ytdl import engine, or second audio engine.
