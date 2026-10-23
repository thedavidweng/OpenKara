# ADR 0038 — Preview NetEase tracks by streaming without a library song

Date: 2026-10-19
Status: accepted

## Context

A user browsing a NetEase playlist could only import a track before hearing
it. ADR 0034 ends every Streaming Import at the shared import path, and the
playback engine and queue assume a library song. A preview that waits for a full
download and import is slow, and importing every track just to listen would
fill the library.

## Decision

Double-click or Enter on a playable row streams the track as an Online Preview.
A preview has no library song, no database row, and no file the user keeps.

- The queue id is `ne:<remote track id>`. The frontend keeps title, artist and
  cover for those ids in the catalog store, as it does for `yt:` items. A track
  that is already in the library plays as that library song instead.
- The existing `play` command branches on the `ne:` prefix. A song-less loader
  resolves the audio URL with `song_url`, reads the total size with a one-byte
  range request, and downloads the file with one sequential GET into the
  chunked cache at link speed. The Remote Provider range fetcher asks for one
  64-512 KiB range at a time and pays a round trip per chunk, which starved
  playback on a high-latency link. Decode reads block on bytes that have not
  arrived. Playback starts once about three seconds of audio are on disk
  (at least 256 KiB). A seek past the downloaded frontier restarts the GET with
  `Range: bytes=offset-`; a seek inside downloaded data is instant. The
  coordinator is keyed by id string, so pause, resume, seek and volume work
  unchanged.
- Only a full-length URL plays. Trial clips, tracks without play rights and
  empty URLs are not previewable. A double-click on such a row shows the
  reason and starts nothing.
- A preview is never preloaded, and `set_preload_candidate` ignores `ne:` ids.
  There is no reconnect loop. On a URL expiry or repeated network failure the
  loader re-resolves `song_url` once and swaps the source at the current
  position. A second failure ends playback with a network error event.
- The preview cache lives in `preview-cache` under the app data directory. The
  loader clears it at every preview start, so at most one preview's partial
  file exists.
- Lyrics, separation and stems need a library song. A preview shows a Preview
  badge and an Import to sing action, skips the lyrics fetch, and hides Add
  lyrics.

## Consequences

- One more branch in `play`, and one more queue id family beside `yt:`.
- Remote URLs from NetEase expire. Seeking far after an expiry costs one
  re-resolve, and a second expiry in the same preview stops it.
- The preview does not use a second audio engine and does not try another
  music source after refusal, keeping ADR 0035 and ADR 0037 intact.
- Previews have no library row, so library play counts do not include them.
