# ADR 0020 — Validate execution provider compatibility before separation

Date: 2026-08-01
Status: accepted

## Context

The target platform does not prove that a runtime provider can run on a host.
The runtime artifact, CPU architecture, and host hardware also affect support.
An explicit provider can therefore fail after the user saves the setting.

## Decision

OpenKara uses one capability policy for automatic selection, settings, and
separation. Apple Silicon uses CoreML when the CoreML runtime is present.
Windows uses DirectML only when a hardware D3D12 adapter can create a level
11.0 device. Intel macOS and Linux keep the measured CPU default. Settings
returns both provider choices and compatible providers. An explicit choice that
is not compatible stays visible, but Settings marks it and separation returns a
localized error that asks the user to switch to CPU.

## Consequences

- CPU remains the compatible choice on every supported target.
- Provider lists must match the shipped runtime artifact matrix.
- New providers must add a host capability check, a settings status, and a
  separation regression test.
- Automatic selection does not use a provider that the capability policy has
  rejected.
