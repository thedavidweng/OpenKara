# ADR 0023 — Windows runtime falls back to CPU on load timeout

Date: 2026-08-07
Status: accepted

## Context

ADR 0019 made the Windows default select DirectML only after a DXGI probe found a
non-software adapter that can create a D3D12 device. A Windows Server 2022 virtual
machine on PVE passes that probe. The hypervisor presents a virtual display adapter
that reports D3D12 capability. The DirectML runtime artifact still deadlocks during
`DllMain`. `DirectML.dll` initialises against the virtual adapter and stops responding.

The deadlock hangs the runtime load past the watchdog timeout. The catalog carried only
one active Windows runtime at the time. That runtime was the DirectML build. The host
had no CPU-only runtime to select instead. Every release from 0.11 onward hit this
failure on the same host (#284).

## Decision

Ship a second active Windows runtime artifact. This artifact advertises only the CPU
execution provider. It is built without `-Donnxruntime_USE_DML=ON` and ships no
DirectML companion library. Catalog generations may now list more than one active
runtime per Windows target.

Runtime resolution takes the preferred execution provider and selects the matching
artifact. DirectML preference selects the DirectML build. CPU preference selects the
CPU-only build. A single-match target still resolves as before.

When an active runtime load times out, the host checks whether the loaded artifact
advertised DirectML. If it did, the host records a persisted
`directml_disabled_by_runtime_timeout` marker. It also flips an in-process override so
the same process stops advertising DirectML capability. The next runtime selection on
that host resolves the CPU-only artifact. The startup load no longer probes a freshly
promoted candidate in the parent process. The worker probe is authoritative for
candidates.

The bootstrap status snapshot carries a `cpu_fallback_notice` string when the host runs
the CPU-only runtime because of a recorded DirectML timeout. The frontend shows this
notice as a one-time toast.

## Consequences

- A Windows host whose adapter passes DXGI but deadlocks DirectML recovers on the next
  restart. It selects the CPU-only runtime.
- An explicit user execution-provider choice overrides the timeout disable. A user can
  still force DirectML after a timeout.
- Catalog consumers must resolve runtimes with the preferred execution provider. Older
  single-runtime catalogs keep working unchanged.
- ADR 0019 capability probing stays in place. The timeout disable is an additional
  signal layered on top of it.
