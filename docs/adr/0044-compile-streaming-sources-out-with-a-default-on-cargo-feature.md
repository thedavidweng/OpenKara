# ADR 0044 — Compile Streaming Sources out with a default-on Cargo feature

Date: 2026-10-31
Status: accepted

## Context

[ADR 0041](./0041-run-streaming-sources-as-sandboxed-packages-from-user-added-registries.md) runs Streaming Sources as packages that users download from Source Registries. App store policies can reject an app that downloads and runs music-service code. iOS forbids it. Flathub review can also reject it. A store rejection must not block a release. The frontend is one bundle for all builds. A frontend build flag would need a second frontend build for each store variant, and the flag could disagree with the Rust build.

## Decision

Put the source host, the application adapter, every Streaming Source command and event, the `source_registry_link` module and the deep-link plugin behind the Cargo feature `streaming-sources`. The feature is on by default. Store builds use `--no-default-features` and the config overlay `src-tauri/tauri.no-streaming-sources.conf.json`, which removes the `openkara` scheme registration. The command `streaming_sources_supported` always exists and tells the frontend which build it runs in. The frontend asks once at startup. It hides the Streaming Sources surface and does not call other Streaming Source commands when the answer is `false`.

## Consequences

- Mac App Store and iOS builds always compile Streaming Sources out. An iOS build with the feature on fails to compile.
- Flatpak keeps Streaming Sources. A variant without them is documented for the case where Flathub rejects them.
- The same frontend bundle serves both builds. New Streaming Source UI must check the catalog store support flag before it calls a Streaming Source command.
- New Streaming Source commands, events and engine-only dependencies must go behind the feature. CI runs Clippy in both feature states.
- Commands that local video and file reveal use are not part of the feature, because local songs need them.
- A build without the feature does not read or delete Streaming Source data. A later build with the feature finds that data again.
