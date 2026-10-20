# Catalog Contract

Field or command semantic changes must update this document before changing UI
code. Domain terms come from `CONTEXT.md`. Decisions: ADR 0031–0036.

The independently installed Streaming Source process boundary is specified in
[Streaming Source protocol 1.0](streaming-source-protocol.md) and ADR 0038.
That contract does not change the current application IPC below; adoption is
the source-neutral host/browser slice of #551.

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
  "refusal": null,
  "library_song_id": null,
  "cover_url": "https://p1.music.126.net/cover.jpg"
}
```

`StreamingTrack.cover_url` is the album cover as an HTTPS URL without a size
parameter, or `null`. The provider's `http://` URLs are rewritten to `https://`.

`StreamingTrack.library_song_id` is the song id of the library song this track
was imported as (from its persisted Streaming Track Identity), or `null`.

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
  "track_count": 12,
  "cover_url": "https://p1.music.126.net/playlist.jpg"
}
```

`StreamingPlaylistDetail`

```json
{
  "remote_id": "pl-1",
  "name": "Night set",
  "tracks": [],
  "cover_url": "https://p1.music.126.net/playlist.jpg",
  "creator": "Ada",
  "description": "Late set",
  "track_count": 12,
  "updated_at_ms": 1700000000000,
  "track_ids": ["1", "2"],
  "partial": false
}
```

`creator`, `description`, `cover_url`, and `updated_at_ms` are `null` when the
provider omits them. `track_count` is the provider's total, which can exceed
`tracks.length` only if provider metadata for some tracks is missing.
`track_ids` lists every track of the playlist in order. `partial` is `true`
when `tracks` holds only the first batch of `track_ids`; the rest is fetched
with `get_streaming_tracks`.

`StreamingTrackListing` (liked tracks)

```json
{ "tracks": [], "track_ids": ["1", "2"], "partial": true }
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

`StreamingImportTrackState` is tagged by `kind`:

- `{ "kind": "queued" }`
- `{ "kind": "downloading", "percent": 62 }` (`percent` is 0-100 and omitted when unknown)
- `{ "kind": "importing" }`
- `{ "kind": "imported", "song_id": "hash" }`
- `{ "kind": "skipped", "refusal": ImportRefusal }`
- `{ "kind": "failed", "reason": StreamingImportFailureReason }`

`StreamingImportEvent`

```json
{
  "job_id": "import-1",
  "remote_track_id": "123",
  "title": "Title",
  "artist": "Artist",
  "state": { "kind": "queued" },
  "completed": 0,
  "total": 12
}
```

`StreamingImportJobSnapshot`

```json
{
  "job_id": "import-1",
  "phase": "running",
  "completed": 0,
  "total": 12,
  "tracks": [
    {
      "remote_track_id": "123",
      "title": "Title",
      "artist": "Artist",
      "state": { "kind": "queued" }
    }
  ]
}
```

`StreamingImportJobSnapshot.phase`: `"running"` | `"awaiting_decision"` | `"completed"`

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
6. `list_streaming_liked_tracks(source_id: OnlineSourceId) -> StreamingTrackListing`
7. `list_streaming_playlists(source_id: OnlineSourceId) -> Vec<StreamingPlaylistSummary>`
8. `get_streaming_playlist(source_id: OnlineSourceId, remote_playlist_id: String) -> StreamingPlaylistDetail`
9. `get_streaming_tracks(source_id: OnlineSourceId, remote_track_ids: Vec<String>) -> Vec<StreamingTrack>`
10. `search_streaming_source(source_id: OnlineSourceId, query: String) -> Vec<StreamingTrack>`
11. `start_streaming_import(source_id: OnlineSourceId, remote_track_ids: Vec<String>, remote_playlist_id: Option<String>) -> StreamingImportProgress`
12. `continue_streaming_import(action: LibraryDecisionAction) -> StreamingImportProgress`
13. `cancel_streaming_import() -> ()`
14. `get_streaming_import_status() -> Option<StreamingImportJobSnapshot>`
15. `resolve_video_source_url(source_id: OnlineSourceId, url: String) -> Vec<VideoQueueItem>`
16. `get_reveal_targets(song_id: String) -> RevealTargets`
17. `reveal_in_folder(path: String) -> ()`

## Events

- `netease-sign-in-complete` targets `main` with a verified `StreamingSessionSnapshot`.
- `netease-sign-in-error` targets `main` with a `CommandError` and no credentials.
- `streaming-import-progress` targets `main` with a `StreamingImportEvent` for each
  per-track state change of the running Streaming Import.

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
    Each request line carries an `id` that the response echoes, so the one
    sidecar process runs up to four requests concurrently and the host matches
    responses by id. A worker exit fails every waiting request and the process
    is respawned on the next use.
    A missing sidecar reports an integration error. Other provider rejections,
    including code 8860, remain `code: streaming_auth_failed` with the
    provider code; they do not trigger an alternative identity or source.
    Playlist details use the complete `trackIds` list for `track_ids`.
12. Resolve of a Streaming Track returns an importable temp file or an
    Import Refusal. A no-play-rights song, trial clip, or empty stream URL is an
    Import Refusal. A pricing flag alone does not refuse an available full URL. An Import Refusal does not download and does not fetch a
    replacement from another platform. Provider audio-format names are
    case-insensitive; downloaded files use lowercase extensions.
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
22. `start_streaming_import` and `continue_streaming_import` run off the main
    thread; the promise resolves when the import completes or pauses for a
    Library Decision. Only one import runs at a time; starting another while one
    is running or awaiting a decision fails. Each track reports `queued`, then
    `downloading` (with `percent` when the response length is known), then
    `importing`, then `imported`, `skipped` (Import Refusal) or `failed`.
    `completed` counts tracks in a final state. NetEase downloads stream to the
    temp file in chunks and report `percent` on each whole-percent change; the
    percent is omitted only when the response has no `Content-Length`.
23. `cancel_streaming_import` stops the import before the next track and
    interrupts a download in flight between chunks, deleting the partial temp
    file. Pending and interrupted tracks are reported as
    `failed` with reason `cancelled` and appear on the failure list. While a
    Library Decision is open the flag applies when the decision resumes.
24. `get_streaming_import_status` returns the latest job snapshot, or `null`
    before any import, so the UI recovers state after a remount.
25. `library_song_id` is filled for liked, playlist and search results. It
    survives restarts because Streaming Track Identity is persisted and is
    removed with its song.
26. Cover images load from `https://*.music.126.net`. The WebView CSP allows that
    origin in `img-src` only; no other directive (including `connect-src`) lists
    it. The UI requests sized thumbnails by appending `?param=<w>y<h>`.
27. Provider rights flags are matched to songs by `privilege.id === song.id`.
    Array position is used only when the provider returned privileges without
    ids. A song with no matching privilege is not refused. Each `song_detail`
    batch logs a debug-level summary of `st`, `pl`, and `fee` counts so genuine
    refusals can be told apart from a mismatch.
    Listing-time refusal follows YesPlayMusic's `isTrackPlayable`: `pl > 0`
    (or `cs`) is playable; otherwise `fee 1` is `trial_clip` (VIP only, served
    as a 30-second clip), `fee 4` (paid album), a present `noCopyrightRcmd`, or
    `st < 0` is `no_play_rights`. The UI names the reason in the skipped list;
    the song title and artist are filled for single and selected imports.
28. `list_streaming_liked_tracks`, `list_streaming_playlists`,
    `get_streaming_playlist`, `get_streaming_tracks`, `search_streaming_source`,
    `sign_out_streaming_source`, and `resolve_video_source_url` run their
    provider calls off the main thread and never block the UI.
29. `get_streaming_playlist` returns immediately with the first batch of tracks
    and rights flags that `playlist_detail` already carries, every track id, and
    `partial: true` when more remain. `list_streaming_liked_tracks` returns the
    first 100 liked tracks the same way. The UI fetches the remaining ids with
    `get_streaming_tracks` in pages of 500, three at a time, and the list grows
    in id order as pages arrive. `start_streaming_import` with a playlist id
    resolves every track of the playlist, not only the loaded ones.
30. `song_detail` results are kept in an in-memory cache keyed by track id for
    the session (bounded at 20000 tracks, cleared on sign-in and sign-out), so
    reopening a list or scrolling never refetches. Fetch timings are logged at
    debug level.
31. An Online Preview uses the queue id `ne:<remote track id>`. The `play`
    command accepts it and streams the track without a library song: it
    resolves `song_url`, rejects trial clips and refused tracks with a
    playback error, then plays from one sequential download into the cache
    (about three seconds buffered before start; a seek past the downloaded
    frontier restarts the download at that offset). Pause,
    resume, seek and volume behave as for library songs. `ne:` ids are not
    in the library database, so lyrics, stems and history do not apply.
32. `set_preload_candidate` is a no-op for `ne:` ids. A preview does not
    reconnect: on URL expiry or repeated network failure it re-resolves
    `song_url` once and continues at the current position. A second failure,
    or a server without range support, emits `PLAYBACK_ERROR_EVENT` with
    `NetworkUnavailable`. The `preview-cache` folder in the app data directory
    is cleared at every preview start.
