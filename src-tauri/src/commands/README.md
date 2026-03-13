# commands

Tauri command handlers and the thin service functions that back them. Keep the
pure import/query logic testable without spinning up a Tauri window, and keep
the command wrappers limited to state access and error translation.
