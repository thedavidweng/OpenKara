# commands

Tauri IPC adapters. Domain write path for the local library lives in `crate::library`
(import, delete, song metadata, playlists). Command modules bind `AppState`, open the
DB via `cache`, and use `remote::PublishChanges` for remote refresh, outbox,
publish, and recovery. Keep pure library logic out of this layer.
