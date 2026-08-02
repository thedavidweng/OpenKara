# ADR 0019 — Execution provider selection uses host capability

Date: 2026-08-01
Status: accepted

## Context

The Windows runtime artifact includes the DirectML execution provider. Some
Windows hosts have no hardware adapter that can create a D3D12 device. A
Windows virtual machine can have this state even when the runtime artifact is
valid. The old default selected DirectML on every Windows host. Runtime
initialisation also loaded every DLL beside the ONNX Runtime library. This
could start DirectML on a host that could not use it and block runtime
bootstrap.

## Decision

The Windows automatic default must select DirectML only after DXGI finds a
non-software adapter that can create a D3D12 device at feature level 11.0. It
must select CPU when this check fails. A saved provider remains an explicit
user choice. Runtime initialisation loads only the ONNX Runtime library.
OpenKara preloads the exact DirectML companion path only before a DirectML
session is created.

## Consequences

- CPU-only Windows hosts do not initialise DirectML during runtime bootstrap.
- Capable Windows hosts use the hardware DirectML path by default.
- DirectML remains available as an explicit Windows setting.
- The capability check selects a usable path. It does not benchmark providers.
