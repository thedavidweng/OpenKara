# ADR 0022 — Windows runtime load uses the install directory

Date: 2026-08-06
Status: accepted

## Context

Catalog ONNX Runtime installs under the app data directory, not next to the
OpenKara executable. On Windows, loading `onnxruntime.dll` without that install
directory in the DLL search path can resolve companion libraries such as
`DirectML.dll` against the wrong location. First-install probe can then hang or
fail after a successful download. A separate worker process runs install and
probe so a hang does not freeze the main UI. The worker is the same GUI
subsystem binary as the app.

## Decision

Before `ort::init_from` on Windows, set the process DLL search directory to the
runtime install folder that contains `onnxruntime.dll`. Leave that directory
set for the process lifetime; only one runtime can commit. Stage a verified
install before probe so a timed-out probe can retry activation without a second
download. Spawn the runtime worker with `CREATE_NO_WINDOW` on Windows. Write
probe start and result lines to the worker stderr file so timeout errors can
include the stalled path and phase.

## Consequences

- Windows runtime load does not depend on the app install directory for
  companion DLLs next to the catalog library.
- Retries after post-download timeout prefer a staged verified install when
  present.
- Timeout errors may include worker stderr and phase for diagnosis. Localized
  recovery copy still comes from the app language (see ADR 0021).
- DirectML companion preload for inference sessions remains separate (ADR 0019).
