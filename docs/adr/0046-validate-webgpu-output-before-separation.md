# ADR 0046 — Validate WebGPU output before separation

Date: 2026-11-03
Status: accepted

## Context

GPU drivers can return incorrect output without an error. On one Windows host,
DirectML reset the GTX 965M and returned non-finite values on the Intel HD 530.
WebGPU output on the HD 530 also differed from CPU output. The GTX 965M
matched the CPU within 0.000273 in full-song pre-encoding PCM.
For the first validation window, raw tensor maximum absolute errors were
0.000789 on Apple M3, 0.000795 on GTX 965M, and 1.513075 on HD 530.

## Decision

Use the official ONNX Runtime and WebGPU plugin. Request a high-performance
adapter. Before a GPU runs a model for the first time, compare both raw output
tensors for one real audio window against CPU. Require finite values and a
maximum absolute difference at most 0.001. Save the result against the adapter,
driver, runtime, plugin, and model identity. Use CPU and report the reason when
validation fails. A change of identity requires a new comparison. Cache the
compiled CoreML models using the model cache key.

Apple Silicon tries WebGPU, CoreML, then CPU; Intel Macs try CoreML then CPU.
On the measured Apple Silicon host, cached CoreML took 1973.54 seconds for a
four-stem song, compared with 29.73 seconds for WebGPU. Windows tries WebGPU
then CPU, and Linux uses CPU. A numerical validation failure goes directly to CPU.

The official Windows runtime replaces the static-CRT build from ADR 0025.
OpenKara no longer requires a custom Windows runtime build.

## Consequences

- DirectML is no longer a selectable provider.
- GPU validation adds one CPU and one GPU inference on first use.
- A failed validation remains in force until the identity changes.
