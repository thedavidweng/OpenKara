# Queue Management

**Last updated:** 2026-05-13  
**Covers:** Queue panel, up-next, play-now/play-next/add-to-queue, drag reorder, auto-advance, clear, and keyboard shortcuts.

## Scope

This spec covers the user-facing behaviour of the playback queue. It does **not** cover:

- Underlying queue data structure or IPC commands (see [`../contracts/phase-2-playback-contract.md`](../contracts/phase-2-playback-contract.md))
- Singers or rotation (see planned `F1-*.md`)
- CDG, AirPlay, or fullscreen audience out-of-band state

## Adding songs to the queue

### Play Now

- Clears the current queue and immediately starts playing the selected song.
- If a song is already playing, that track is stopped, the new song starts from position 0.
- If playback was paused, the new song starts playing (transitions from paused → playing).
- The previously-playing song is **not** re-added to the queue.

### Play Next

- Inserts the selected song at position 0 (immediately after the currently-playing song).
- If the queue is empty, the song is added as the first entry and auto-advance will play it when the current song ends.
- Multiple "Play Next" invocations stack in insertion order: first invocation is position 0, second is position 1 (both before any existing queue entries).

### Add to Queue

- Appends the selected song to the end of the current queue.
- If playback is idle (no song loaded, queue empty), the song starts immediately.
- Multiple "Add to Queue" invocations append in call order.

### Context menu

These actions are available from:

- Song context menu in the library (right-click / long-press on any song row)
- Batch mode: selecting multiple songs shows "Queue All Selected (N)" with the same Play Next / Add to Queue semantics applied in library sort order.

## Queue panel

### UI structure

- The queue panel opens via the queue button in the toolbar (top-right region).
- Panel displays the list of queued songs in order, with the currently-playing song highlighted.
- Each queue entry shows: cover art thumbnail, title, artist, duration.
- Drag handle on each entry for reorder.

### Current song indicator

- The currently-playing song is shown at the top of the queue with a "Now Playing" badge.
- Its position in the queue does not change while playing.
- If the current song finishes and auto-advances, the next entry becomes "Now Playing" and is removed from the queue listing.

### Drag reorder

- Users can drag any queued entry (except the currently-playing song) to a new position.
- Drag feedback shows the insertion line between entries.
- Accessibility: keyboard reordering via focus → Space/Enter to pick up → Arrow keys to move → Space/Enter to drop (with live region announcements for screen readers per `src/locales/en.json` keys "dragStart", "dragOverBefore", etc.).

### Remove from queue

- Each entry (except current song) has a remove button (X icon).
- Removing the current song stops playback and advances to the next queued song or idle.
- "Clear All" button at the top of the panel removes every song except the currently-playing one.

## Auto-advance

- When the current song reaches its end, the queue auto-advances to the next entry.
- The next entry starts playing automatically; no user action required.
- After the last queue entry finishes, playback transitions to idle:
  - `isPlaying` becomes `false`
  - `songId` becomes `null`
  - UI shows "Select a song to start" in the lyrics panel.
- Auto-advance respects crossfade: if enabled (future feature), the next song begins fading in before the current song ends. (Not yet implemented — crossfade is out of scope for this spec version.)

## Queue and library deletion

- Deleting a song from the library does **not** automatically remove it from the queue.
- If the deleted song is currently playing, playback continues until the track ends or the user skips.
- After the track ends, the deleted song does **not** auto-advance (it has no file to load), and playback transitions to idle.
- If the deleted song is queued but not playing, it remains in the queue listing but will fail to load when auto-advanced. The error is surfaced as a toast: "Song not found."
- The queue is **not** persisted across app restarts (intentional design choice for v0.8.x — may change in future).

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

- "Play Now" from library always works regardless of queue state.
- Clear All on an already-empty queue is a no-op (no error, no UI change).
- Queue panel shows "Queue is empty" message when no songs are queued.

### Single-song loop

- When only one song is in the queue and it ends, playback loops the same song (repeat-one behaviour).
- This is consistent with the playback state machine where a single-song queue with repeat enabled restarts the same track.

### Rapid operations

- Rapid successive Play Next + Remove can produce a brief race where a song plays for a fraction of a second before being removed. This is acceptable; the next queued song (or idle) resolves within one event loop tick.
- Drag reorder during playback does not interrupt audio.

## i18n keys

New keys added for queue management (see `src/locales/en.json`):

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
