# ADR 0031 — Keep Online Sources distinct from Remote Providers

Date: 2026-08-16
Status: accepted

## Context

OpenKara must let a user enable YouTube and NetEase Cloud Music. Those
origins look like plugins. The glossary already uses Remote Provider for
Drive, Dropbox, and WebDAV. Those hosts store a Remote Repository. A
music service does not store the OpenKara library. A Last.fm Scrobbler
does not supply audio. UnblockNeteaseMusic looks like a third music
origin, but it has no account and no catalog. One trait for search,
login, download, and video play would mix these jobs.

## Decision

Treat an Online Source as a Streaming Source or a Video Source. A
Streaming Source is an account-backed music service. The first adapter
is NetEase Cloud Music. Later adapters include Kugou and QQ Music. A
Video Source resolves a public link into queue items. The first adapter
is YouTube. A Scrobbler is not an Online Source. A Remote Provider is
not an Online Source. UnblockNeteaseMusic is not an Online Source.

## Consequences

- Settings expose an Online Source switch. The switch is not a
  repository action.
- A new streaming brand adds a Streaming Source adapter. It does not
  add a new import pipeline.
- A new "paste a video link" origin adds a Video Source adapter. It
  does not add download.
- Callers must not name NetEase or YouTube a Remote Provider.
- Last.fm and ListenBrainz wait for a Scrobbler ADR.
