# commands

Tauri IPC adapters. Domain write path for the local library lives in `crate::library`
(import, delete, song metadata, playlists). Command modules bind `AppState`, open the
DB via `cache`, and wrap remote Pre-Mutation Refresh / Publish Song hooks from
`remote_library::run_*_mutation`. Keep pure library logic out of this layer.
