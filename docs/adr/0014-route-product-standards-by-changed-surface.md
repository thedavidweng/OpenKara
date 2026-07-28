# ADR 0014 — Route product standards by changed surface

Date: 2026-07-28
Status: accepted

## Context

ADR 0013 selects product quality standards. A single reference page gives
agents too much unrelated material. An agent can also miss the standards route
when `AGENTS.md` does not name it.

## Decision

`AGENTS.md` contains a short mandatory route to
[`docs/references/product-standards.md`](../references/product-standards.md).
The page maps changed product surfaces to small profile documents in
`docs/references/standards/`.

An implementation reads the index and only its matching profiles. A profile
states its authorities, constraints, and required evidence. The pull request
records that evidence or a standard-specific exception. A new product surface
or a changed conformance target needs an ADR.

`check:standards` verifies the routing structure in local hooks and CI. It
protects the route. It does not replace feature-level tests or human review.

## Consequences

- Every future agent receives the standards route without loading the full
  standards reference.
- Standard guidance stays close to the surface that uses it.
- The repository claims conformance only where it has matching evidence.
- A broken standards route fails before merge.
