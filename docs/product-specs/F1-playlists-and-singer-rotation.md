# F1 — Playlists & Singer Rotation

**Last updated:** 2026-05-13  
**Status:** Approved (maintainer-attested for implementation, 2026-05-13)  
**Spec acceptance:** Per [`../plan/plan.md`](../plan/plan.md) F1 spec acceptance — maintainer has confirmed via execution instruction: all 6 mandatory sections present and no open SPEC gap comments.

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

Singer assignment is available from:

1. **Queue panel** — each queued entry shows a singer name field (if rotation is active)
2. **Playlist view** — "Assign Singer" per song in the playlist context menu
3. **Now playing** — click the singer name/badge to change

Singer names are free-form text (no registration required). The app remembers the last N singer names for autocomplete (capped at 50, stored in `library_meta` as JSON array).

## 2. Rotation Rules

### Round-robin definition

Given singers `[Alice, Bob, Carol]` and a playlist with songs `[S1, S2, S3, S4, S5]`:

1. Song S1 is assigned to Alice.
2. When S1 ends or is skipped, the queue advances to S2 (assigned to Bob).
3. When S2 ends → S3 assigned to Carol.
4. When S3 ends → S4 assigned to Alice (wrap-around).
5. Continue until the playlist is exhausted.

### Manual advance

- User can click "Next Singer" to skip the current singer's turn.
- Manual advance moves to the next singer in rotation without playing a song.
- Empty queue + manual advance is a no-op (no state change).

### Mid-song singer change

- Changing the singer mid-song updates the current queue entry's singer field.
- The rotation pointer does **not** advance (the same singer keeps their turn for the next song after this one finishes).

### Empty queue

- If the queue is empty, rotation state is preserved. When songs are added, rotation resumes from the current pointer.
- If all singers are deleted (rotation reset), the pointer resets to position 0.

### Single-singer

- Rotation with one singer always assigns all songs to that singer.
- No round-robin or wrap-around applies.

### Wrap-around

- When the last singer in the list has been assigned a song and the queue still has more entries, the pointer wraps to the first singer.

## 3. Library Track Removal

### Playlist behavior

When a song is removed from the library:

1. **Immediate** — The song is removed from all playlists (SQL `ON DELETE CASCADE` or application-level cleanup on library mutation).
2. **Best-effort** — If the playlist contained the song as its only entry, the playlist remains (empty). If the playlist contained the song as "Now Playing" in rotation, the queue auto-advances to the next song.
3. **No silent data loss** — The user sees a toast after batch library deletion: "Removed N songs from M playlists."

### Queue behavior

- Deleting a song from the library does not remove it from the queue.
- If the deleted song is currently playing, playback continues until the track ends.
- After track ends, the deleted song does not auto-advance (load failure → toast: "Song not found").
- If the deleted song is queued but not playing, it remains in the queue listing but will fail to play when reached.

### Rotation state

- If a singer has no songs remaining in the queue or playlist, the rotation advances past them on the next turn.
- If all singers have no songs, rotation pauses (idle state).

## 4. Concurrency / Single-Writer

- **Single-writer assumption:** The app assumes one active UI window and no concurrent automation. Opening multiple windows or sending rapid-fire commands from scripts is **not supported** and may produce undefined rotation state.
- **Future-proofing:** The SQLite schema uses `BEGIN IMMEDIATE` transactions so concurrent writers at the database layer are safe (last-writer-wins for queue/rotation metadata). No optimistic locking is implemented for the playlist metadata layer.
- **Rapid operations:** Repeated add/remove within the same event loop tick are serialized by the frontend store; the backend sees them as sequential mutations.

## 5. Migration & Failure

### SQLite migrations

- Playlist/singer tables will be added in a **new migration** (`008_playlists.sql` and `009_singer_rotation.sql`).
- Migrations are **forward-only**. If a migration fails (disk full, constraint violation):
  1. The transaction is rolled back.
  2. The app shows a toast: "Failed to update database. Your library is unchanged."
  3. The user is advised to back up their library and retry.
- **Rollback:** Not supported. If the user downgrades to an older app version, the new tables will simply be ignored (the old code won't query them). No data loss.

### Failure modes

| Failure                     | User-visible                                                        | Recovery                                                                            |
| --------------------------- | ------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| Migration SQL error         | Toast: "Failed to update database."                                 | Backup + retry; if persistent, file a GitHub issue.                                 |
| Disk full during save       | Toast: "Could not save playlist. Free up disk space and try again." | Free space → retry.                                                                 |
| Concurrent edit conflict    | Last-writer-wins — the latest save overwrites silently.             | None needed; data is eventually consistent.                                         |
| Corrupt playlist data in DB | Toast: "Playlist data could not be read."                           | The playlist section shows empty; corruption is logged. Manual restore from backup. |

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

## Data Model (for implementer reference)

### New tables

```sql
-- 008_playlists.sql
CREATE TABLE IF NOT EXISTS playlists (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS playlist_songs (
    playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    song_hash   TEXT NOT NULL REFERENCES songs(hash) ON DELETE CASCADE,
    added_at    INTEGER NOT NULL,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    singer      TEXT,
    PRIMARY KEY (playlist_id, song_hash)
);

-- 009_singer_rotation.sql
CREATE TABLE IF NOT EXISTS rotation_state (
    id              INTEGER PRIMARY KEY DEFAULT 1,
    singer_names    TEXT NOT NULL DEFAULT '[]',       -- JSON array of strings
    current_index   INTEGER NOT NULL DEFAULT 0,
    mode            TEXT NOT NULL DEFAULT 'round_robin',  -- 'round_robin' | 'single'
    active          INTEGER NOT NULL DEFAULT 0        -- boolean
);
```

### IPC commands (planned)

| Command                      | Direction          | Purpose                                    |
| ---------------------------- | ------------------ | ------------------------------------------ |
| `list_playlists`             | Backend → Frontend | Returns all playlists with song counts     |
| `create_playlist`            | Frontend → Backend | Creates a new named playlist               |
| `rename_playlist`            | Frontend → Backend | Renames an existing playlist               |
| `delete_playlist`            | Frontend → Backend | Deletes a playlist and its entries         |
| `add_songs_to_playlist`      | Frontend → Backend | Adds songs to a playlist                   |
| `remove_songs_from_playlist` | Frontend → Backend | Removes songs from a playlist              |
| `get_playlist_songs`         | Frontend → Backend | Returns songs in a playlist                |
| `set_rotation_state`         | Frontend → Backend | Sets singer names and mode                 |
| `get_rotation_state`         | Frontend → Backend | Returns current rotation config            |
| `advance_rotation`           | Frontend → Backend | Advances rotation pointer (manual next)    |
| `set_queue_entry_singer`     | Frontend → Backend | Sets the singer for a specific queue entry |

## Implementation Sequence

1. **Spec approval** — This doc checked and approved per `plan.md` F1 spec acceptance.
2. **Data layer** — Migrations `008_playlists.sql`, `009_singer_rotation.sql` + Rust commands.
3. **Contracts** — Update `phase-1-library-contract.md` and playback contract.
4. **i18n** — Add string keys to `en.json` and `zh-CN.json`.
5. **Frontend playlist panel** — Sidebar section + playlist view + CRUD modals.
6. **Singer rotation UI** — Queue singer assignment + rotation controls.
7. **Tests** — Rust tests for persistence, rotation rules, library-track removal. Vitest for frontend store actions.
8. **H8.6 gate** — Verify `check-i18n.mjs` passes with new keys.
