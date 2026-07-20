# Playlists & Singer Rotation

---

## 1. UX State Machine

### Screens / Modals

```
┌────────────────────────────────────────────────────────┐
│  Main sidebar:  "PLAYLISTS" section                    │
│  ┌──────────────────────────────────────────────────┐  │
│  │ [+ New Playlist]                                 │  │
│  │──────────────────────────────────────────────────│  │
│  │  My Favorites         (3)   ⋮                    │  │
│  │  Jazz Night           (12)  ⋮                    │  │
│  │  Duet Practice        (5)   ⋮                    │  │
│  └──────────────────────────────────────────────────┘  │
│                                                        │
│  When a playlist is selected:                          │
│  ┌──────────────────────────────────────────────────┐  │
│  │ ← Back to All Playlists    Edit  Delete  [Add]  │  │
│  │─────────────────────────────────────────────────│  │
│  │ ♫ Song A — Artist A                          ☰  │  │
│  │ ♫ Song B — Artist B                          ☰  │  │
│  │ ...                                            │  │
│  └──────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────┘
```

**Playlist CRUD modals:**

| Action       | Trigger                                | Behavior                                                                               |
| ------------ | -------------------------------------- | -------------------------------------------------------------------------------------- |
| **Create**   | "[+] New Playlist" button              | Inline text input: "Playlist name…" + Create/Cancel. Default name "New Playlist".      |
| **Rename**   | Context menu "Rename" (⋮)              | Inline rename (click on name → edit). Enter or blur saves.                             |
| **Delete**   | Context menu "Delete" (⋮)              | Confirm dialog: "Delete 'Playlist Name'? Songs will not be removed from your library." |
| **Add song** | Library context menu "Add to Playlist" | Submenu listing all playlists with checkmark next to ones containing the song.         |

### Singer assignment (rotation)

You can assign a singer from:

1. **Queue panel** — each queued entry shows an "Assign Singer" button when rotation is active
2. **Queue panel header** — singer chips select the current singer. "Next Singer" advances the rotation pointer.
3. **Playlist view** — "Assign Singer" per song in the playlist context menu
4. **Now playing** — click the singer name or badge to change it

Singer names are free-form text. No registration is required. The app remembers the last N singer names for autocomplete. N is capped at 50. The names are stored in `library_meta` as a JSON array.

## 2. Rotation Rules

### Round-robin definition

Given singers `[Alice, Bob, Carol]` and a playlist with songs `[S1, S2, S3, S4, S5]`:

1. Song S1 is assigned to Alice.
2. When S1 ends or the user skips it, the queue advances to S2. S2 is assigned to Bob.
3. When S2 ends, S3 is assigned to Carol.
4. When S3 ends, S4 is assigned to Alice (wrap-around).
5. Continue until the playlist is exhausted.

### Manual advance

- The user can click "Next Singer" to skip the current singer's turn.
- Manual advance moves to the next singer in rotation. It does not play a song.
- Empty queue and manual advance is a no-op. No state change occurs.

### Mid-song singer change

- If the user changes the singer mid-song, the current queue entry's singer field updates.
- The rotation pointer does **not** advance. The same singer keeps their turn for the next song after this one finishes.

### Empty queue

- If the queue is empty, rotation state is preserved. When the user adds songs, rotation resumes from the current pointer.
- If the user deletes all singers (rotation reset), the pointer resets to position 0.

### Single-singer

- Rotation with one singer always assigns all songs to that singer.
- No round-robin or wrap-around applies.

### Wrap-around

- When the last singer in the list gets a song and the queue still has more entries, the pointer wraps to the first singer.

## 3. Library Track Removal

### Playlist behavior

When a song is removed from the library:

1. **Immediate** — The song is removed from all playlists. This uses SQL `ON DELETE CASCADE` or application-level cleanup on library mutation.
2. **Best-effort** — If the song was the only entry, the playlist remains empty. If the song was "Now Playing" in rotation, the queue auto-advances to the next song.
3. **No silent data loss** — After batch library deletion, the user sees a toast: "Removed N songs from M playlists."

### Queue behavior

- If the user deletes a song from the library, the queue keeps it.
- If the deleted song is currently playing, playback continues until the track ends.
- After the track ends, the deleted song does not auto-advance. A load failure shows a toast: "Song not found."
- If the deleted song is queued but not playing, it stays in the queue listing. It will fail to play when reached.

### Rotation state

- If a singer has no songs left in the queue or playlist, the rotation advances past that singer on the next turn.
- If all singers have no songs, rotation pauses (idle state).

## 4. Concurrency / Single-Writer

- **Single-writer assumption:** The app assumes one active UI window and no concurrent automation. Multiple windows or rapid-fire commands from scripts are **not supported**. They may produce undefined rotation state.
- **Concurrency:** The SQLite schema uses `BEGIN IMMEDIATE` transactions. Concurrent writers at the database layer are safe (last-writer-wins for queue/rotation metadata). The playlist metadata layer does not implement optimistic locking.
- **Rapid operations:** Repeated add/remove within the same event loop tick are serialized by the frontend store. The backend sees them as sequential mutations.

## 5. Migration & Failure

### SQLite migrations

- Playlist and singer tables are in migrations `008_playlists.sql` and `009_singer_rotation.sql`.
- Migrations are **forward-only**. If a migration fails (disk full, constraint violation):
  1. The transaction rolls back.
  2. The app shows a toast: "Failed to update database. Your library is unchanged."
  3. The user must back up the library and retry.
- **Rollback:** Not supported. If the user downgrades to an older app version, the new tables are ignored. The old code does not query them. No data loss occurs.

### Failure modes

| Failure                     | User-visible                                                        | Recovery                                                                                     |
| --------------------------- | ------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| Migration SQL error         | Toast: "Failed to update database."                                 | Backup + retry; if persistent, file a GitHub issue.                                          |
| Disk full during save       | Toast: "Could not save playlist. Free up disk space and try again." | Free space → retry.                                                                          |
| Concurrent edit conflict    | Last-writer-wins — the latest save overwrites silently.             | None needed; data is eventually consistent.                                                  |
| Corrupt playlist data in DB | Toast: "Playlist data could not be read."                           | The playlist section shows empty. The app logs the corruption. Restore manually from backup. |

## 6. i18n

### New string keys

```
playlist.create          → Create playlist
playlist.name            → Playlist name
playlist.rename          → Rename
playlist.delete          → Delete
playlist.deleteConfirm   → Delete "{{name}}"?
playlist.deleteMessage   → Songs will not be removed from your library.
playlist.empty           → This playlist is empty.
playlist.addTo           → Add to Playlist
playlist.removedToast    → Removed {{count}} songs from {{playlistCount}} playlists.
rotation.assignSinger    → Assign Singer
rotation.singer          → Singer
rotation.nextSinger      → Next Singer
rotation.roundRobin      → Round-robin
rotation.singleSinger    → Single singer
rotation.noSinger        → No singer assigned
```

### en copy

| Key                      | en                                                           |
| ------------------------ | ------------------------------------------------------------ |
| `playlist.create`        | New Playlist                                                 |
| `playlist.name`          | Playlist name                                                |
| `playlist.rename`        | Rename                                                       |
| `playlist.delete`        | Delete Playlist                                              |
| `playlist.deleteConfirm` | Delete "{{name}}"?                                           |
| `playlist.deleteMessage` | Songs in the playlist will not be removed from your library. |
| `playlist.empty`         | This playlist is empty. Add songs from your library.         |
| `playlist.addTo`         | Add to Playlist…                                             |
| `playlist.removedToast`  | Removed {{count}} songs from {{playlistCount}} playlists.    |
| `rotation.assignSinger`  | Assign Singer                                                |
| `rotation.singer`        | Singer                                                       |
| `rotation.nextSinger`    | Next Singer                                                  |
| `rotation.roundRobin`    | Round-robin                                                  |
| `rotation.singleSinger`  | Single singer                                                |
| `rotation.noSinger`      | No singer assigned                                           |
| `playlist.section`       | PLAYLISTS                                                    |

### zh-CN copy

| Key                      | zh-CN                                                        |
| ------------------------ | ------------------------------------------------------------ |
| `playlist.create`        | 新建播放列表                                                 |
| `playlist.name`          | 播放列表名称                                                 |
| `playlist.rename`        | 重命名                                                       |
| `playlist.delete`        | 删除播放列表                                                 |
| `playlist.deleteConfirm` | 删除"{{name}}"?                                              |
| `playlist.deleteMessage` | 播放列表中的歌曲不会从曲库中删除。                           |
| `playlist.empty`         | 这个播放列表是空的。请从曲库中添加歌曲。                     |
| `playlist.addTo`         | 添加到播放列表…                                              |
| `playlist.removedToast`  | 已从 {{playlistCount}} 个播放列表中移除了 {{count}} 首歌曲。 |
| `rotation.assignSinger`  | 指定歌手                                                     |
| `rotation.singer`        | 歌手                                                         |
| `rotation.nextSinger`    | 下一位歌手                                                   |
| `rotation.roundRobin`    | 轮唱                                                         |
| `rotation.singleSinger`  | 单人演唱                                                     |
| `rotation.noSinger`      | 未指定歌手                                                   |
| `playlist.section`       | 播放列表                                                     |

Schema and IPC commands for playlists and rotation are in
[`../contracts/library.md`](../contracts/library.md) and the generated
[`../generated/db-schema.md`](../generated/db-schema.md).
