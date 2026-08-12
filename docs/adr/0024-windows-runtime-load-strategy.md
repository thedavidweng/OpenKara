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

The most probable cause: our runtime artifacts are built with `/MD` (catalog
`toolchain.cmake_flags` carries `CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL`), so
`onnxruntime.dll` imports the Microsoft Visual C++ Redistributable. The PE import
tables of the shipped artifacts (cpu-reduced and directml-reduced builds alike) name
exactly four redistributable DLLs:

- `vcruntime140.dll`
- `vcruntime140_1.dll`
- `msvcp140.dll`
- `msvcp140_1.dll`

Everything else they import is a Windows inbox component (`kernel32`, `advapi32`,
`setupapi`, `dbghelp`, `dxgi`, and the `api-ms-win-crt-*` UCRT api-sets, all present
since Windows 10), and `DirectML.dll` — which ships inside the runtime artifact and
links the CRT statically, so it adds no redistributable dependency. Rust's MSVC target
links only the UCRT, so `openkara.exe` itself runs fine without the redistributable.
A stripped PVE Windows Server 2022 image ships without it; loading `onnxruntime.dll`
then fails with `ERROR_MOD_NOT_FOUND (126)`.

## Decision

The Windows runtime load uses three cooperating mechanisms. None replaces the others;
each closes a different gap exposed by #284.

### 1. App-local VC++ CRT DLLs installed next to `openkara.exe`

The Windows installer places the four CRT DLLs directly beside `openkara.exe`
(`tauri.windows.conf.json` maps each file in `src-tauri/resources/windows/vcredist/`
to the install root). The application directory is the first entry in the Windows
loader's standard DLL search order, so every load of `onnxruntime.dll` — startup
activation, staged-candidate activation, the bootstrap worker's probe, and the load
before separation — resolves the CRT from the app install without any runtime code.

This placement is the deciding property, and why the DLLs do not live in each runtime
artifact directory instead:

- Runtime loads happen on several paths that never run the installer/worker code
  (`begin_startup`, active-slot activation, `try_activate_staged_runtime`). Per-install
  staging only covers installs performed after the staging code shipped; a user with an
  already-installed runtime — exactly the #284 reporter — would keep failing until a
  reinstall. Files placed by the app installer cover every runtime directory, past and
  future, the moment the app updates.
- No migration or staging logic at all: the installer and updater own the files'
  lifecycle, and one copy serves any number of runtime artifacts.
- It is the app-local deployment model Microsoft documents for the redistributable:
  colocate the CRT with the executable, no elevation, no reboot, works where a redist
  installer cannot run.

The DLLs are extracted from the official `vc_redist.x64.exe` (14.44.35211.0) and
committed to the repo (~768KB). `resources/windows/vcredist/manifest.json` pins each
file's SHA-256 and size; the `windows_vcredist_resources` integration test enforces on
every host that the committed binaries match the pinned digests, that the manifest
lists exactly the four DLLs from the import table, that the Windows bundle config maps
each one to the install root, and that no other platform bundles them.

### 2. Probe-load with `GetLastError` capture

`init_ort_from_path` calls `LoadLibraryExW` itself before `ort::init_from`. On failure
it reads `GetLastError` and maps the code to a human description via
`describe_win32_load_error`. `ort`/`libloading` drop the OS error in their `Display`
impl, so without this probe an instant `LoadLibraryExW` failure surfaces as the opaque
"LoadLibraryExW failed" string and the user cannot act on it.

The probe replicates the real load exactly — `libloading`'s `Library::new` is
`LoadLibraryExW(path, NULL, 0)`, so the probe passes the same null handle and zero
flags. Both loads therefore resolve dependencies through the same standard search
order (application directory first, then the `SetDllDirectoryW` runtime directory that
carries `DirectML.dll`), and the probe fails exactly when the real load would. An
earlier draft used `LOAD_WITH_ALTERED_SEARCH_PATH`, which substitutes the DLL's own
directory for the application directory and would have diverged from `ort`'s load.

On success the probe frees the module again. Its reference count can reach zero there,
in which case Windows unloads it and `ort::init_from` performs a fresh load with the
normal loader work and `DllMain` execution. The probe therefore supersedes the v0.13.2
`fs::read` prewarm rather than the load itself: it pays the first-touch disk reads and
antivirus scan under our control instead of under the watchdog, and it exercises the
full import resolution that `fs::read` never reached.

### 3. Persisted CPU fallback and elevated watchdog (unchanged)

ADR 0023's CPU-only runtime fallback and the 120-second watchdog from v0.13.2 stay in
place. They cover hosts where the load genuinely hangs (slow virtual disk, AV scan,
DirectML deadlock) rather than failing instantly.

## Consequences

- A Windows image missing the VC++ Redistributable loads the runtime after simply
  updating the app; existing runtime installs need no reinstall and no migration. The
  cost is ~768KB of redistributable Microsoft binaries committed to the repo and
  shipped in the Windows installer only.
- A future runtime-load failure shows the real `GetLastError` code in the log and UI
  (e.g. `Win32 error 126 / 0x0000007E`), narrowing diagnosis without another release.
- The probe adds one extra `LoadLibraryExW`/`FreeLibrary` cycle per runtime activation.
  On the success path that is a second pass of loader work for `ort` — cheap, because
  the file cache is warm by then — and on the failure path it short-circuits before the
  watchdog can trip.
- Servicing: the app-local CRT is pinned, not serviced by Windows Update. Refreshing it
  means dropping newer official DLLs into `resources/windows/vcredist/` and re-pinning
  the manifest; the integration test keeps the three surfaces in lockstep. If a future
  runtime artifact adds a CRT import (e.g. `concrt140.dll`), the test's required-set
  assertion is the place that fails first.
- The strategic simplification — building the runtime artifacts with a static CRT
  (`/MT`) in `openkara-models` so no redistributable is needed at all — is out of this
  repository's reach and tracked separately.
