# Catalog Contract

The source process boundary is specified in [Streaming Source protocol 1.0](streaming-source-protocol.md). Production source discovery uses the host registry. Developer mocks are UI-only fixtures and never replace production discovery or process execution.

## Identity and lifecycle

`OnlineSourceId` is an open string. Source ids use safe lowercase ASCII. The native `list_online_sources()` registry contains only YouTube. `set_online_source_enabled(source_id, enabled)` changes its `AppSettings.youtube_source_enabled` flag. Streaming Sources use independent lifecycle commands, not AppSettings flags.

`SourceSnapshot` contains `source_id`, host-derived `owner_binding`, `name`, `description`, `license`, nullable `installed_version`, `available_version`, `staged_version`, `pinned_version`, `enabled`, `runtime_ready`, `running`, `state`, nullable `progress`, nullable `last_error`, `active_work`, and nullable `manifest`.

`state` is `not_installed`, `installing`, `ready`, `disabled`, `update_required`, `unavailable`, `waiting_for_idle`, or `cancelling`. Installation, enablement, runtime readiness and sign-in are separate states. `progress` contains `phase`, `completed`, and nullable `total`. `last_error` is `{code,message,retryable}`. Active work contains numeric `id`, `kind` (`authentication`, `preview`, or `import`), and `cancelling`.

`SourceManifest` contains `manifest_version`, `source_id`, `package_id`, `publisher`, `version`, `minimum_host_version`, `protocol: {majors:number[],minor:number}`, `runtime: {kind:string,major:number}`, `entry`, `name`, `description`, `license`, string arrays `capabilities`, `requires_authentication`, `resource_origins`, `media_origins`, and the protocol-defined `authentication` value.

| Command                                                                      | Result             |
| ---------------------------------------------------------------------------- | ------------------ |
| `list_streaming_sources()`                                                   | `SourceSnapshot[]` |
| `refresh_streaming_sources()`                                                | `SourceSnapshot[]` |
| `install_streaming_source(source_id, version=null)`                          | `SourceSnapshot`   |
| `cancel_streaming_source_install(source_id)`                                 | void               |
| `set_streaming_source_enabled(source_id, enabled, stop_work=false)`          | `SourceSnapshot`   |
| `uninstall_streaming_source(source_id, stop_work=false, remove_credentials)` | `SourceSnapshot`   |
| `pin_streaming_source(source_id, version=null)`                              | `SourceSnapshot`   |

Disable and uninstall refuse active work unless `stop_work` is explicit. A stop cancels source-bound work and waits for it to stop before removing resources. Uninstall requires an explicit credential-retention choice. A failed refresh remains visible and does not erase prior snapshots. `streaming-source-status` carries the changed `SourceSnapshot`.

## Authentication

`get_streaming_session(source_id)` returns `{source_id,signed_in,display_name,expired}` after plugin session verification, not a storage-presence guess. `sign_out_streaming_source(source_id,stop_work=false)` clears that source's saved session and returns the same shape. Sign-out requires explicit stop consent when source work is active.

`open_streaming_sign_in(source_id)` returns `AuthChallenge`: `{source_id,challenge,expires_at,poll_after_ms,presentation}`. `poll_after_ms` is nullable. Presentation is `{kind:'web'}`, `{kind:'qr',content,text}`, or `{kind:'text',text}`.

`poll_streaming_sign_in(source_id,challenge)` returns `{state,poll_after_ms?,session?}`. State is `pending`, `complete`, `expired`, or `cancelled`. `cancel_streaming_sign_in(source_id,challenge)` stops that challenge. Challenges belong to one source.

Web sign-in uses host-controlled windows and declared cookie capture. Credentials never cross frontend IPC. `streaming-sign-in-complete` carries the verified session. `streaming-sign-in-error` carries `{source_id,challenge,error}` without credentials. A consumer ignores errors from an earlier challenge. Cancellation compares and removes only the originating challenge. Polling can also observe completion.

## Catalog pages and resources

`StreamingTrack` contains `source_id`, `remote_track_id`, `title`, `artist`, nullable `album`, `duration_ms`, `refusal`, `library_song_id`, and `cover_url`. Artwork URLs are host-managed resource handles. The frontend must not append upstream image-resize parameters or infer provider origins.

`StreamingTrackListing` is `{tracks:StreamingTrack[],next_cursor:string|null}`. `StreamingPlaylistSummary` contains `source_id`, `remote_id`, `name`, nullable `track_count`, and nullable `cover_url`. `StreamingPlaylistListing` is `{items:StreamingPlaylistSummary[],next_cursor:string|null}`. `StreamingPlaylistDetail` contains `source_id`, `remote_id`, `name`, `tracks`, nullable `cover_url`, `description`, `track_count`, `creator`, `updated_at_ms`, and `next_cursor`.

| Command                                                            | Result                     |
| ------------------------------------------------------------------ | -------------------------- |
| `list_streaming_liked_tracks(source_id,cursor=null)`               | `StreamingTrackListing`    |
| `search_streaming_source(source_id,query,cursor=null)`             | `StreamingTrackListing`    |
| `list_streaming_playlists(source_id,cursor=null)`                  | `StreamingPlaylistListing` |
| `get_streaming_playlist(source_id,remote_playlist_id,cursor=null)` | `StreamingPlaylistDetail`  |
| `get_streaming_tracks(source_id,remote_track_ids)`                 | `StreamingTrack[]`         |

Cursors are opaque and bound to the source and listing. A null next cursor ends pagination. Remote ids can collide across sources; identity is always `(source_id,remote_id)`.

## Import and preview

`start_streaming_import(source_id,remote_track_ids,remote_playlist_id,job_id)` and `continue_streaming_import(action)` return `StreamingImportProgress`. The caller creates a unique `job_id` and stores the source/job pair before invoking start. All subsequent progress must match both identifiers, including the first event while start remains pending. `remote_playlist_id` is nullable. `cancel_streaming_import()` cancels the global job. `get_streaming_import_status()` returns its latest `StreamingImportJobSnapshot` or null. One global job runs at a time; its source cannot change when selection changes.

Progress, job snapshots and `streaming-import-progress` events all contain `source_id`. Progress contains `status` (`awaiting_decision` or `completed`), `imported_song_ids`, `failed`, nullable `playlist_id`, and nullable `conflict`. Job snapshots contain `job_id`, `phase` (`running`, `awaiting_decision`, `completed`), `completed`, `total`, and `tracks`. Events contain `job_id`, `remote_track_id`, `title`, `artist`, `state`, `completed`, and `total`.

`streaming-import-complete` carries `{source_id,job_id,progress:StreamingImportProgress}` for terminal outcomes, including cancellation of a parked decision and source revocation. The global consumer must match both identifiers, clear the pending conflict, and finish the import run even when another source or view is active. A late event from another source or job must not change the current import.

Track states are `queued`, `downloading` (optional percent), `importing`, `imported` (song_id), `skipped` (refusal), or `failed` (reason). Failure reasons are `refusal`, `cancelled`, and `import_failed`. Refusal reasons are `no_play_rights`, `trial_clip`, `empty_url`, and `unavailable`. Protocol `no_media` maps to `empty_url`. A refusal never substitutes another source. Successful imports become ordinary library songs. Playlist origin and track identity include the source id. Conflict actions are `keep`, `replace`, `apply_keep`, `apply_replace`, or `cancel`.

Online Preview queue ids are `stream:${source_id}:${encodeURIComponent(remote_id)}`. Metadata maps use the complete canonical id. Preview ids are not library songs and do not gain library lyrics, stems, or history. Host lyric lookup uses source-bound metadata. Preview work belongs to its source and must stop before source removal.

`resolve_video_source_url(source_id,url)` returns `VideoQueueItem[]` for native YouTube links. Video queue ids retain `yt:`. It does not use YouTube `/player` stream URLs. `get_reveal_targets(song_id)` returns `song_file` and `stems` targets, each `{available,path}`. `reveal_in_folder(path)` opens an existing target in the system file manager.

## Browser and Settings UI

Navigation uses installed source snapshots, not a fixed brand list. Settings
keeps discovery, installation, enablement, runtime readiness, and verified
authentication separate. The built-in YouTube preference controls only the
native Video Source. A failed source refresh keeps existing snapshots usable.
The local-library view is separate from the active source id. A valid source
id such as `library` does not alias local-library navigation.

The browser shows liked tracks for `liked_tracks`, search for `search`, and
playlists only when both `playlists` and `playlist_tracks` are declared. Import
and preview controls require their own capabilities. A library-song mapping
takes precedence over Online Preview. Authentication gates only methods named
by `requires_authentication`; public methods remain available without sign-in.

Each source owns its session, query, cursors, loaded pages, selection, errors,
and request generations. Delayed results update the source that started the
request. Navigation does not cancel an import or move its task subscriptions.
The import bar and Library Decision dialog remain available outside a source
browser. Import events must match both source and caller-created job id.

`streaming-import-complete` carries `{source_id,job_id,progress}` after a running
or paused import reaches its terminal result. Runtime subscriptions consume it
outside navigation and clear the matching Library Decision. Cancellation from
revocation settles a paused conflict before the work lease is released. A late
Keep or Replace decision cannot override completed cancellation.

Import uses metadata already returned by that source before it requests optional
track details. A public import does not gain a sign-in requirement from an
optional protected detail method. Missing requested track details fail explicitly;
the host does not report omitted identities as imported. Failed or rejected
artwork remains unavailable artwork after host validation, not a failed track.

Official-page completion retains the source/challenge terminal result for polling
after the host closes its window and releases active work. New sign-in and
sign-out clear the retained result.

Load More forwards the exact opaque cursor. Playlist import starts the global
source-bound backend job before it follows playlist cursors. Preparation holds
the actual import work lease and supports cancellation outside navigation.
An empty track selection with a playlist id means the complete playlist;
the backend enumerates its pages and reports track progress through the shared
job. The browser never requests pages by a provider-specific id batch size.
Artwork paths use the host asset protocol without CDN rewrites. Artwork loads
observe the same work cancellation token as import preparation.

Tabs support Arrow Left, Arrow Right, Home, and End with a roving tabindex.
The Find shortcut focuses the supported search field. Track rows retain Enter
playback, selection, select-all, shift-range selection, refusal explanations,
and local-song preference. Loading failures provide a retry action and do not
present a failed request as an empty collection.

Authentication renders plugin text as text. QR content is encoded locally as
an SVG QR code, not loaded as plugin HTML, SVG, or a remote image. Polling and
cancellation stay bound to the source challenge. Disable, sign-out, and
uninstall explain active source work and require explicit consent to stop it.
Uninstall also requires an explicit keep-or-remove credential choice; neither
choice removes local songs or playlists.
