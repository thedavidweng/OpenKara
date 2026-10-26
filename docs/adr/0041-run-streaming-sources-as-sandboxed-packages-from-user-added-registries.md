# ADR 0041 — Run Streaming Sources as sandboxed packages from user-added registries

Date: 2026-10-26
Status: accepted

## Context

ADR 0039 runs each Streaming Source as a managed Node process. Clients download a pinned Node runtime and a package with a large dependency tree. Each release needs project signatures, a signed source index, key custody and revocations. A source process has full user privileges, so a bad package can read files and other secrets. Phone builds cannot start processes. Store builds need a variant without downloadable music-service code. This decision implements the design in [#589](https://github.com/thedavidweng/OpenKara/issues/589).

## Decision

Supersede the ADR 0039 decisions about distribution, signing, runtime and transport. Keep the ADR 0039 state model, credential rules and update-consent rules. Keep its HTTPS-only media descriptions. Keep host ownership of windows, downloads, Online Preview and Streaming Import.

Run each enabled Streaming Source in QuickJS through the `rquickjs` crate, inside the host process. Give each source its own runtime and context on its own thread. Sources share no globals. Disable `eval` and the `Function` constructor. The only code that a source runs is its verified entry file. Enforce the 10-second `describe` deadline and the 30-second operation deadline through the interrupt hook. Set a memory limit and a stack limit for each source. When a source hits a limit, stop it and return `source_unavailable`. The next call starts the source fresh, with no restart backoff.

A package is one ES module entry file and its manifest. The manifest runtime kind is `openkara-js` with major 1. The package lists protocol major 2. The protocol keeps the methods, data shapes, capabilities, authentication flows, cursors and error codes of protocol 1.0. Only the transport and the runtime change. The host API is the only capability that a package has: HTTPS requests to declared origins, crypto primitives, a small key-value store, timers and logging.

OpenKara ships with no Streaming Source and no Source Registry. Users add a Source Registry by HTTPS URL or by an `openkara://` link. Each registry entry records the package size and SHA-512 integrity. The host checks the bytes before it loads them. Remove the signed source index, the source-signing keys and their custody, revocations, and the rule that only project-controlled releases install. Remove the managed Node runtime download, pins, Flatpak Node bundling and the macOS runtime quarantine regression. Remove the stdio worker process, its crash-restart backoff and the protocol 1.x stdio transport.

## Consequences

- Host tests need no Node runtime and no network. `cargo test` works on a fresh clone.
- A package cannot read files, start processes, open sockets or contact undeclared origins. The sandbox, the origin rule and credential namespaces together are the security boundary.
- A runaway source cannot freeze OpenKara. A source that hits a limit loses its in-flight calls.
- An installed package identity is the pair of Source Registry and package id. Only one installed package can hold a source id.
- Node-based installations need replacement. Their runtime, package files and Streaming Credentials go away. Library songs, Streaming Track Identities and Playlist Origin Stamps stay.
- No parallel compatibility path remains after the migration. Remove the Node runtime and the stdio transport when the migration ships.
- The OpenKara maintainer no longer signs source releases or holds source-signing keys.
