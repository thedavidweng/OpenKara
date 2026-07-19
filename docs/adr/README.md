# Architecture Decision Records

ADRs capture durable decisions and non-obvious constraints that the code
cannot explain on its own. Each record is a short, dated markdown file
numbered in creation order (`NNNN-kebab-title.md`).

## When to add an ADR

Add a record when a decision is:

- **load-bearing** — changing it would break correctness, portability, or a
  frozen contract, and
- **non-obvious** — a reader of the code alone would not reconstruct the
  reasoning (e.g. why a specific `ort` feature flag is pinned, why a cursor
  is monotonic, why two resampler caches must not be shared).

Do not add an ADR for anything the code already says clearly, for release
history (that lives in `CHANGELOG.md`), or for frozen IPC interfaces (those
live in `docs/references/contracts/`).

## Format

```markdown
# ADR NNNN — <imperative title>

Date: YYYY-MM-DD
Status: accepted | superseded by NNNN | deprecated

## Context

<one paragraph: the problem and the forces that make it non-obvious>

## Decision

<one paragraph: what we chose>

## Consequences

<bullet list: what this forces on future code, and what it rules out>
```

Supersede a record by writing a new one and updating the old `Status` line —
do not delete or rewrite accepted records.

## Index

- [0001 — ADR format and scope](./0001-adr-format-and-scope.md)
