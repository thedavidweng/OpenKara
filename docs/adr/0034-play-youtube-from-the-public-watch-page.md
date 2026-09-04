# ADR 0034 — Play YouTube from the public watch page

Date: 2026-08-16
Status: accepted

## Context

Many karaoke videos already contain accompaniment and on-screen lyrics.
The user wants to paste a YouTube or playlist link into the OpenKara
queue. Kaset plays DRM media in a WebView. It does not download audio.
Kaset verified on 2026-07-01 that a signed-out `/player` call returns
UNPLAYABLE and no streamingData. Public watch pages and public playlist
browse still work. Google sign-in would unlock age gates and private
lists. It would also require a cookie jar, SAPISIDHASH, account
switching, and a Google session in OpenKara. OpenKara is not a YouTube
Music client.

## Decision

YouTube is a Video Source. Resolve a public watch or playlist link into
queue items. Play each item by loading the public watch page in a
WebView. Do not call `/player` stream URLs. Do not store Google
cookies. Do not import YouTube audio. Do not put YouTube items in a
Playlist. Age-restricted, private, or unlisted items fail in the open.
Only one YouTube player may run. The audience window owns it when that
window is open.

## Consequences

- Queue identity may use a `yt:` prefix. That prefix cannot collide
  with a song hash.
- Stem controls, EQ, crossfade, waveform, lyrics acquisition, and
  AirPlay do not apply to a YouTube item.
- A later Google sign-in is a new product surface.
- Playback must stop the local audio engine before the YouTube WebView
  starts.
