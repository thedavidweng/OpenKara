# ADR 0025 — Windows runtimes link the MSVC CRT statically

Date: 2026-08-13
Status: accepted

## Context

ADR 0024 shipped four app-local VC++ CRT DLLs beside `openkara.exe`. The
`/MD`-built `onnxruntime.dll` imported them. A Windows image without the VC++
Redistributable failed the load with `ERROR_MOD_NOT_FOUND` (#284). That
deployment was a mitigation. The repo carried ~768 KB of pinned Microsoft
binaries. CRT servicing was manual (#363). The strategic fix landed upstream.
`openkara-models` PR #90 builds both Windows runtime targets with the static
CRT: `CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded` plus the matching
`ONNX_USE_MSVC_STATIC_RUNTIME`, `protobuf_MSVC_STATIC_RUNTIME`, and
`ABSL_MSVC_STATIC_RUNTIME` flags. Catalog generation 13 (release
`2026-08-12-001`) publishes those artifacts. Their PE import tables carry no
`VCRUNTIME140*` or `MSVCP140*` entries. Every import is a Windows inbox
component or `DirectML.dll`. `DirectML.dll` ships inside the runtime artifact.

## Decision

OpenKara consumes the static-CRT runtimes from catalog generation 13. OpenKara
removes the app-local CRT deployment from ADR 0024: the
`src-tauri/resources/windows/vcredist/` binaries and manifest, the
`windows_vcredist_resources` integration test, the Windows bundle resource
mappings, and the installed-layout CRT assertions in
`reusable-windows-installed-app.yml`. This supersedes decision 1 of ADR 0024.
Decisions 2 and 3 stay. The probe-load with `GetLastError` capture and the
persisted CPU fallback with the elevated watchdog address failure modes that
do not depend on the CRT linkage.

## Consequences

- Generation-13 runtime loads do not depend on the VC++ Redistributable.
  Existing installs pick up generation 13 through catalog auto-discovery. That
  path needs no app release and no reinstall.
- The repo carries no Microsoft binaries and does no CRT servicing.
- CI runners ship the redistributable, so the installed-app smoke cannot detect
  a regression to `/MD` artifacts. The guard lives upstream: `openkara-models`
  pins the static-CRT flags in its source lock, and the catalog `toolchain`
  metadata records them per artifact. A future runtime bump must keep
  `CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded` on the Windows targets.
- The `ERROR_MOD_NOT_FOUND` hint in `describe_win32_load_error` points at
  runtime download integrity and the bundled `DirectML.dll`, not at the CRT. A
  machine that still fails with error 126 on an old runtime generation needs
  the generation-13 artifacts, not the redistributable.
