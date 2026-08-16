# Catalog Contract

Field or command semantic changes must update this document before changing UI
code. Domain terms come from `CONTEXT.md`. Decisions: ADR 0031–0034.

This version only exposes the Online Source registry. Browse, sign-in, import,
and YouTube resolve land in later changes on the same contract.

## Types

`OnlineSourceId`: `"youtube"` | `"netease"`

`OnlineSourceKind`: `"video"` | `"streaming"`

`OnlineSourceSnapshot`

```json
{
  "id": "youtube",
  "kind": "video",
  "enabled": false
}
```

## Commands

1. `list_online_sources() -> Vec<OnlineSourceSnapshot>`
2. `set_online_source_enabled(source_id: OnlineSourceId, enabled: bool) -> AppSettings`

## Semantics

1. The registry always returns YouTube then NetEase, in that order.
2. Both sources default to `enabled: false`.
3. `set_online_source_enabled` persists only that source. It does not sign out
   and it does not clear Streaming Credentials.
4. An unknown `source_id` returns `CommandError` with `code: internal`.
5. A disabled source must reject later catalog commands. Those commands are not
   in this version.
6. `AppSettings.youtube_source_enabled` and `AppSettings.netease_source_enabled`
   match the registry flags after every successful set or `get_settings`.
