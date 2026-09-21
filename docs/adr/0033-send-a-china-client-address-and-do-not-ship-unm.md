# ADR 0033 — Send a China Client Address and do not ship UNM

Date: 2026-08-16
Status: accepted

## Context

NetEase often returns an empty stream URL when the client egress IP is
outside mainland China. Unofficial clients send `X-Real-IP` with a
mainland address. YesPlayMusic and ncm-api-rs already support this as
`real_ip` or `random_cn_ip`. UnblockNeteaseMusic is a different
product. It searches other platforms when NetEase refuses a full track.
That path is GPL-3.0. OpenKara is Apache-2.0. Grey songs and trial
clips are copyright refusals, not geo refusals.

## Decision

The NetEase adapter always sends a China Client Address on its API
requests. Do this the same way YesPlayMusic does. Do not add an UNM
engine. Do not add ytdl, Bilibili, or other replacement sources in this
version. Do not expose a Real-IP setting. An Import Refusal stays an
Import Refusal after the China Client Address is sent.

## Consequences

- Overseas users can import tracks that NetEase already permits in
  China.
- A grey song or a trial clip still fails. The failure list names the
  refusal.
- A PR that links unm_engine or server-rust is out of scope.
- A later "search this title on another signed-in Streaming Source"
  feature is a new product surface.
