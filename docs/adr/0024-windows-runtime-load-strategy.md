# ADR 0024 — Windows runtime load strategy

Date: 2026-08-11
Status: accepted

## Context

Issue #284 tracked a Windows Server 2022 (PVE/KVM) host where the ONNX Runtime DLL
failed to load. Earlier releases addressed two adjacent failure modes:

- ADR 0023 added a CPU-only runtime artifact and a DirectML timeout fallback, after the
  DirectML build deadlocked inside `DllMain` on the virtual adapter.
- v0.13.2 added a best-effort `fs::read` prewarm and raised the load watchdog timeout,
  on the hypothesis that real-time Defender scanning or a cold virtual disk was stalling
  the loader under the watchdog.

After v0.13.2, the user disabled Defender real-time monitoring and ran the app as
Administrator. The failure changed from a 60-second watchdog timeout to an **instant**
`LoadLibraryExW failed` (~850ms in the log). Two conclusions followed:

1. The Defender/cold-disk hypothesis was wrong for this host. Disabling Defender did not
   fix the load; it changed the failure. The prewarm did its job (DLL bytes are now
   resident), so the loader reached the real failure fast instead of stalling.
2. `LoadLibraryExW` returned NULL immediately. `ort` wraps `libloading`, whose `Display`
   impl drops the OS error code and prints only `"LoadLibraryExW failed"`. The actual
   `GetLastError` value never reached the log or the user, so we could not tell why.

The most probable cause: `onnxruntime.dll` is built with `/MD` (catalog
`toolchain.cmake_flags` carries `MultiThreadedDLL`), so it imports `vcruntime140.dll`
and `msvcp140.dll` from the Microsoft Visual C++ Redistributable. Rust's MSVC target
links only `ucrtbase.dll`, a standard Windows component, so `openkara.exe` runs fine
even when the VC++ Redistributable is absent. A stripped PVE Windows Server 2022 image
ships without that redistributable. Loading `onnxruntime.dll` then fails with
`ERROR_MOD_NOT_FOUND (126)`.

## Decision

The Windows runtime load uses three cooperating mechanisms. None replaces the others;
each closes a different gap exposed by #284.

### 1. App-local VC++ runtime DLL bundling

Ship `vcruntime140.dll`, `vcruntime140_1.dll`, and `msvcp140.dll` as Tauri bundle
resources (`src-tauri/resources/windows/vcredist`). At runtime install time
(`install_runtime_with_verified_archive_cache`), copy them next to `onnxruntime.dll`
in the freshly extracted staging directory before the atomic activation rename.

This is the documented Microsoft deployment model for desktop apps. It needs no
elevation, no reboot, no system state, and works in locked-down environments where a
redist installer cannot run. The total cost is ~732KB uncompressed, ~300-400KB after
NSIS LZMA compression.

Every bundled DLL is verified against a pinned `manifest.json` (SHA-256 + size) before
copy. A mismatch or a missing file aborts the install; we never copy an unverified blob,
even one shipped with the app, so a corrupted or tampered resource fails loudly instead
of loading into the separation process.

### 2. Probe-load with `GetLastError` capture

`init_ort_from_path` calls `LoadLibraryExW` itself before `ort::init_from`. On success
it frees the module immediately; the subsequent `ort` load is a refcount bump. On
failure it reads `GetLastError` and maps the code to a human description via
`describe_win32_load_error`. `ort`/`libloading` drop the OS error in their `Display`
impl, so without this probe an instant `LoadLibraryExW` failure surfaces as the opaque
"LoadLibraryExW failed" string and the user cannot act on it.

The probe also supersedes the v0.13.2 `fs::read` prewarm. Running `DllMain` settles any
antivirus scan and warms the loader cache under our control rather than the watchdog.

### 3. Persisted CPU fallback and elevated watchdog (unchanged)

ADR 0023's CPU-only runtime fallback and the 120-second watchdog from v0.13.2 stay in
place. They cover hosts where the load genuinely hangs (slow virtual disk, AV scan,
DirectML deadlock) rather than failing instantly.

## Consequences

- A Windows image missing the VC++ Redistributable can now load the runtime without the
  user installing anything. The cost is ~732KB of redistributable Microsoft binaries
  committed to the repo and shipped in every Windows installer.
- A future runtime-load failure shows the real `GetLastError` code in the log and UI
  (e.g. `Win32 error 126 / 0x0000007E`), narrowing diagnosis without another release.
- The probe adds one extra `LoadLibraryExW`/`FreeLibrary` cycle per runtime activation.
  On the success path the cost is negligible (refcount bump); on the failure path it
  short-circuits before the watchdog can trip.
- Adding a new VC++ DLL later (e.g. `concrt140.dll`) means adding it to
  `resources/windows/vcredist`, appending to `manifest.json`, and bumping nothing else.
  The staging code reads the manifest; it is not hard-coded to a specific file list.
- This decision is Windows-specific. The staging helper is gated by `cfg(target_os =
"windows")`; the verification core (`stage_vcredist_from_dir`) is platform-independent
  so it can be unit-tested on any host.
