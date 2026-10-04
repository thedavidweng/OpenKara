# ADR 0031 — DirectML graph fusion opt-out

Date: 2026-09-28
Status: accepted

## Context

On one Windows host (NVIDIA RTX 3070 Ti, driver 32.0.16.1088), DirectML graph
fusion miscompiles the spectral-core htdemucs graph. The fused session returns
stems thousands of times too loud. The session loads without an error, so the
provider chain does not fall back to CPU. The user hears loud noise for every
separated song (Levison/OpenKara#1).

ONNX Runtime has the session config entry `ep.dml.disable_graph_fusion`. With
the entry set to `1`, DirectML output matches CPU output on the affected host.
Inference time increases by about 16%. Unfused DirectML stays about 12 times
faster than CPU.

Other hosts do not show the defect. A change to the default DirectML session
would add the time cost on every Windows host.

## Decision

Add the config key `disable_directml_graph_fusion`. The key is absent by
default, and absent means false. When the key is true, DirectML sessions set
`ep.dml.disable_graph_fusion=1`. The key does not change CPU, XNNPACK, or
CoreML sessions. The Settings screen does not show the key. The user sets the
key in `config.json`.

The session cache key includes the option. A change to the option loads a new
session.

## Consequences

- Default DirectML behavior does not change.
- An affected host can keep GPU separation. The user sets one config key.
- The app does not detect the defect. An affected user must find the key.
- `directml_without_graph_fusion_matches_cpu` requires unfused DirectML stems to
  match CPU stems on Windows hosts with DirectML. Both DirectML runs assert the
  session committed on DirectML before comparing stems, so a CPU fallback cannot
  pass the guard vacuously. The test only reports the fused result, because the
  fused result is correct on most hosts.
