# library

Deep library module: song domain model plus local write path.

## Layout

- `Song` / import result types — stable shapes shared with the SQLite cache and IPC layer
- `import/` — path expand, media ingest into `LibraryRoot`, cover extraction
- `delete` — song row + working-copy / stem file removal (also used by remote mirror)
- `songs` — metadata, instrumental/language flags, properties probe, batch delete
- `playlist` — playlists and singer rotation

## Seams

- **Storage adapter:** `crate::cache` (SQL only)
- **Portable paths:** all file I/O goes through `LibraryRoot` relative paths
- **Remote mutations:** `remote::PublishChanges` owns refresh, outbox, publish, and
  recovery; IPC adapters declare the affected `ChangeScope`

Keep command handlers thin: open connection, optionally wrap remote hooks, call into this module.
