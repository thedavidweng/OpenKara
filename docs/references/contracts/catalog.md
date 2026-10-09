# Catalog Contract

Field or command semantic changes must update this document before changing UI
code. Domain terms come from `CONTEXT.md`. Decisions: ADR 0031–0034.

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

`StreamingQrChallenge`

```json
{
  "key": "unikey",
  "login_url": "https://music.163.com/login?codekey=unikey",
  "qr_svg": "<svg></svg>"
}
```

`StreamingQrStatus`: `"waiting"` | `"scanned"` | `"confirmed"` | `"expired"`

`StreamingQrPoll`

```json
{
  "status": "waiting",
  "session": null
}
```

`StreamingPasswordMethod`: `"phone"` | `"email"`

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
4. `start_streaming_qr_signin(source_id: OnlineSourceId) -> StreamingQrChallenge`
5. `poll_streaming_qr_signin(source_id: OnlineSourceId, key: String) -> StreamingQrPoll`
6. `sign_in_streaming_source(source_id: OnlineSourceId, method: StreamingPasswordMethod, identifier: String, password: String, country_code: Option<String>) -> StreamingSessionSnapshot`
7. `sign_out_streaming_source(source_id: OnlineSourceId) -> StreamingSessionSnapshot`
8. `list_streaming_liked_tracks(source_id: OnlineSourceId) -> Vec<StreamingTrack>`
9. `list_streaming_playlists(source_id: OnlineSourceId) -> Vec<StreamingPlaylistSummary>`
10. `get_streaming_playlist(source_id: OnlineSourceId, remote_playlist_id: String) -> StreamingPlaylistDetail`
11. `search_streaming_source(source_id: OnlineSourceId, query: String) -> Vec<StreamingTrack>`
12. `start_streaming_import(source_id: OnlineSourceId, remote_track_ids: Vec<String>, remote_playlist_id: Option<String>) -> StreamingImportProgress`
13. `continue_streaming_import(action: LibraryDecisionAction) -> StreamingImportProgress`
14. `resolve_video_source_url(source_id: OnlineSourceId, url: String) -> Vec<VideoQueueItem>`
15. `get_reveal_targets(song_id: String) -> RevealTargets`
16. `reveal_in_folder(path: String) -> ()`

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
   keychain service `org.openkara.streaming-source`. They never share storage
   with Repository Credentials. The password and its hash are not stored.
9. Sign-out clears Streaming Credentials. Turning NetEase off does not.
10. An expired or risk-control NetEase session, including HTTP 301, returns
    `code: streaming_session_expired` and `expired: true` on the session
    snapshot. The UI returns to sign-in. That is not an Import Refusal.
11. NetEase requests always send a China Client Address as `X-Real-IP`. There
    is no Real-IP setting.
12. Resolve of a Streaming Track returns an importable temp file or an
    Import Refusal. A grey song, trial clip, or empty stream URL is an
    Import Refusal. An Import Refusal does not download and does not fetch a
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
