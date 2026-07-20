# ADR 0005 — Realtime audio callback is lock-free

Date: 2026-07-19
Status: accepted

## Context

The CPAL audio output callback runs on a realtime thread at high priority.
Taking a mutex, condvar, or allocating inside that callback can block the
thread long enough to cause an audible dropout. The streaming source,
multi-stem decode path, and flush/seek coordination all need to
communicate with the callback, but standard synchronization primitives
are unsafe on a realtime thread.

## Decision

The realtime audio callback never takes a mutex or condvar and never
allocates. Cross-thread coordination with producers, the seek path, and
the flush path uses atomic flags (`AtomicBool`, `AtomicUsize`) and
lock-free ring buffers. Producers/consumers that need to wait use short
`sleep` loops on their own (non-realtime) threads, never condvars that
the realtime thread would have to signal under a lock.

Memory ordering is explicit: for example, `flush_done` is cleared with
`Release` ordering before `needs_flush` is published, so the realtime
thread's `Acquire` read of `needs_flush` cannot observe the new request
without also observing the cleared `flush_done` on weakly-ordered hardware.

## Consequences

- Any new coordination between the realtime callback and other threads
  must use atomics or a lock-free structure, not a `Mutex`/`RwLock`/
  `Condvar`. Adding a blocking primitive to the callback path is a
  regression.
- The callback must not allocate (`Vec::new`, `Box`, `String`, etc.) on
  the render hot path; buffers are pre-allocated and reused.
- Memory ordering on the atomic flags is load-bearing; relaxing it to
  `Relaxed` without analysis can introduce flush/seek races on ARM and
  other weakly-ordered targets.
- The `PlaybackCoordinator` (ADR 0002) keeps the callback off the
  control-plane lock by having the callback only read controller state
  under the controller's own lock — which is acceptable because the
  callback's render path holds that lock for a bounded, allocation-free
  critical section, not a blocking wait.
