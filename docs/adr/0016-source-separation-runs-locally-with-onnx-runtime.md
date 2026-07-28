# ADR 0016 — Source separation runs locally with ONNX Runtime

Date: 2026-07-28
Status: accepted

## Context

Stem separation turns a mixed track into vocals, drums, bass, and other
stems. Cloud separation services exist and need no local compute. They send
user audio to a remote server, charge recurring fees, and stop working
offline. OpenKara is a desktop karaoke app. Its users value privacy, offline
play, and no per-use cost. The separation model is large and the runtime is
platform-specific, so a local path must solve model delivery and runtime
delivery on every supported platform.

## Decision

Source separation runs locally on the user's machine. The backend embeds the
Demucs model as an ONNX file and runs it through ONNX Runtime. The model is
downloaded to the app data directory on first launch and verified with a
pinned SHA-256 checksum. The ONNX Runtime shared library is resolved from
bundled app resources, with a local development fallback under
`src-tauri/generated/onnxruntime/`. Runtime installation is catalog-driven
with staged activation: a candidate runtime is verified before it replaces
the active runtime, and a failed activation rolls back to the previous
verified runtime. No user audio leaves the machine for separation.

## Consequences

- Separation works offline. Code that assumes network access for separation
  is a regression.
- User audio for separation never leaves the machine. A future cloud path
  must be opt-in and must not change the default local behavior.
- The runtime bootstrap is load-bearing. A runtime loaded by the current
  process is never overwritten in place. The candidate, active, and previous
  slots are the only supported activation path.
- Model and runtime delivery add first-launch and platform-specific work.
  The model path boundary in `AGENTS.md` holds: `src-tauri/models/` is a
  development cache only, and shipped builds use the app data directory.
- The stem order is fixed by ADR 0009. This ADR records the local-execution
  decision, not the stem order.
