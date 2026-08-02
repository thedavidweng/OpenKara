# ADR 0018 — Remote Repository modules own complete lifecycles

Date: 2026-08-01
Status: accepted

## Context

Remote Repository changes currently pass through several mutation wrappers,
provider calls, and recovery paths. The immediate path and the restart path
repeat publication rules. Tests replace the publication implementation instead
of exercising the production seam.

## Decision

`RemoteRepositoryLifecycle` owns access and recovery actions. `PublishChanges`
owns local mutation, the atomic outbox, publication, status events, and
recovery. `RemoteContent` owns verified downloads, range streams, cache leases,
and stem-set materialization. A provider port exists only where multiple
adapters implement it. SQLite, the file system, the manifest, and the cache
catalog remain concrete inside these modules.

## Consequences

- Immediate publication and restart recovery use one publication driver.
- Provider adapters share conformance tests.
- Callers do not coordinate locks, outbox rows, control projections, or retry
  transitions.
- Existing Remote Repository data stays compatible.
