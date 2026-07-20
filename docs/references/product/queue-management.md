# Queue Management

**Covers:** Queue panel, up-next, play-now/play-next/add-to-queue, drag reorder, auto-advance, clear, and keyboard shortcuts.

## Scope

This spec covers the user-facing behaviour of the playback queue. It does **not** cover:

- Underlying queue data structure or IPC commands (see [`../contracts/playback.md`](../contracts/playback.md)).
- Singers or rotation (see [`playlists-and-singer-rotation.md`](./playlists-and-singer-rotation.md)).
- CDG, AirPlay, or fullscreen audience out-of-band state.

## Adding songs to the queue

### Play Now

- Clears the current queue. Immediately starts playback of the selected song.
- If a song is already playing, that track stops. The new song starts from position 0.
- If playback was paused, the new song starts to play. It transitions from paused to playing.
- The app does **not** re-add the previous song to the queue.

### Play Next

- Inserts the selected song at position 0. This is immediately after the currently-playing song.
- If the queue is empty, the song is added as the first entry. Auto-advance will play it when the current song ends.
- Multiple "Play Next" invocations stack in insertion order. The first invocation is position 0. The second is position 1. Both come before existing queue entries.

### Add to Queue

- Appends the selected song to the end of the current queue.
- If playback is idle (no song loaded, queue empty), the song starts immediately.
- Multiple "Add to Queue" invocations append in call order.

### Context menu

These actions are available from:

- Song context menu in the library (right-click or long-press on any song row).
- Batch mode: select multiple songs to see "Queue All Selected (N)". It applies the same Play Next or Add to Queue semantics in library sort order.

## Queue panel

### UI structure

- The queue panel opens through the queue button in the toolbar (top-right region).
- The panel shows the list of queued songs in order. The currently-playing song is highlighted.
- Each queue entry shows: cover art thumbnail, title, artist, duration.
- Each entry has a drag handle for reorder.

### Current song indicator

- The currently-playing song is shown at the top of the queue. It has a "Now Playing" badge.
- Its position in the queue does not change during playback.
- If the current song finishes and auto-advances, the next entry becomes "Now Playing". The app removes it from the queue listing.

### Drag reorder

- The user can drag any queued entry (except the currently-playing song) to a new position.
- Drag feedback shows the insertion line between entries.
- Accessibility: keyboard reordering uses focus, then Space/Enter to pick up, then Arrow keys to move, then Space/Enter to drop. Live region announcements for screen readers use `src/locales/en.json` keys "dragStart", "dragOverBefore", and so on.

### Remove from queue

- Each entry (except the current song) has a remove button (X icon).
- If the user removes the current song, playback stops. The queue advances to the next queued song or idle.
- "Clear All" button at the top of the panel removes every song except the currently-playing song.

## Auto-advance

- When the current song reaches its end, the queue auto-advances to the next entry.
- The next entry starts to play automatically. No user action is required.
- After the last queue entry finishes, playback transitions to idle:
  - `isPlaying` becomes `false`
  - `songId` becomes `null`
  - The UI shows "Select a song to start" in the lyrics panel.
- Auto-advance respects crossfade. When crossfade is enabled, the next song fades in before the current song ends.

## Queue and library deletion

- If the user deletes a song from the library, the app does **not** remove it from the queue.
- If the deleted song is currently playing, playback continues until the track ends or the user skips.
- After the track ends, the deleted song does **not** auto-advance. It has no file to load. Playback transitions to idle.
- If the deleted song is queued but not playing, it stays in the queue listing. It will fail to load when auto-advanced. The error shows as a toast: "Song not found."
- The queue is **not** persisted across app restarts. This is an intentional design choice.

## Keyboard shortcuts

| Shortcut       | Action                              |
| -------------- | ----------------------------------- |
| `Space`        | Toggle play/pause                   |
| `ArrowRight`   | Seek forward 5 seconds              |
| `ArrowLeft`    | Seek backward 5 seconds             |
| `Cmd/Ctrl + →` | Next track (advance queue)          |
| `Cmd/Ctrl + ←` | Previous track (restart or go back) |
| `Cmd/Ctrl + L` | Focus library search                |
| `Escape`       | Close queue panel / overlay panels  |

## Edge cases

### Empty queue

- "Play Now" from the library always works. It works regardless of queue state.
- Clear All on an already-empty queue is a no-op. No error occurs. No UI change occurs.
- The queue panel shows "Queue is empty" when no songs are queued.

### Single-song loop

- When only one song is in the queue and it ends, playback loops the same song (repeat-one behaviour).
- This is consistent with the playback state machine. A single-song queue with repeat enabled restarts the same track.

### Rapid operations

- Rapid successive Play Next and Remove can produce a brief race. A song may play for a fraction of a second before the app removes it. This is acceptable. The next queued song or idle resolves within one event loop tick.
- Drag reorder during playback does not interrupt audio.

## i18n keys

New keys for queue management are in `src/locales/en.json`:

| Key path                   | en value                                       |
| -------------------------- | ---------------------------------------------- |
| `queue.title`              | Queue                                          |
| `queue.upNext`             | Up Next                                        |
| `queue.clearAll`           | Clear All                                      |
| `queue.empty`              | Queue is empty                                 |
| `queue.badge`              | 9+                                             |
| `queue.reorder`            | Reorder {{title}}                              |
| `queue.remove`             | Remove {{title}} from queue                    |
| `queue.moveUp`             | Move Up                                        |
| `queue.moveDown`           | Move Down                                      |
| `queue.dragInstructions`   | To reorder the queue...                        |
| `queue.dragStart`          | Picked up {{title}}.                           |
| `queue.dragOverBefore`     | {{title}} will be placed before {{overTitle}}. |
| `queue.dragOverAfter`      | {{title}} will be placed after {{overTitle}}.  |
| `queue.dragEndBefore`      | Dropped {{title}} before {{overTitle}}.        |
| `queue.dragEndAfter`       | Dropped {{title}} after {{overTitle}}.         |
| `queue.dragCancel`         | Queue reordering canceled.                     |
| `library.playNow`          | Play Now                                       |
| `library.playNext`         | Play Next                                      |
| `library.addToQueue`       | Add to Queue                                   |
| `library.queueAllSelected` | Queue All Selected ({{count}})                 |
