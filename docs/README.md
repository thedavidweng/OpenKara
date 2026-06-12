# Documentation

This folder is for docs that help maintain the codebase. It is not the product
backlog and it is not a duplicate release history.

## Where Things Belong

| Need                        | Location                                                                   | Rule                                                                        |
| --------------------------- | -------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| What already shipped        | [`../CHANGELOG.md`](../CHANGELOG.md)                                       | Human-readable version history and completed changes                        |
| Future work / backlog       | [GitHub Project](https://github.com/users/thedavidweng/projects/2/views/1) | Do not mirror backlog tables in this repo                                   |
| Work ready for execution    | [`plans/`](./plans/)                                                       | One plan per implementable work slice, with acceptance and verification     |
| Current facts and contracts | [`references/`](./references/)                                             | Architecture, product behavior, IPC contracts, release rules, testing notes |
| Historical context          | [`archive/`](./archive/)                                                   | Completed or superseded plans, status snapshots, old workflow scripts       |

## Current Entrypoints

| Document                                                                                         | Purpose                           |
| ------------------------------------------------------------------------------------------------ | --------------------------------- |
| [`references/index.md`](./references/index.md)                                                   | Current reference index           |
| [`references/architecture/architecture.md`](./references/architecture/architecture.md)           | System architecture               |
| [`references/architecture/project-structure.md`](./references/architecture/project-structure.md) | Repository layout and ownership   |
| [`references/architecture/releasing.md`](./references/architecture/releasing.md)                 | Release and distribution rules    |
| [`references/contracts/README.md`](./references/contracts/README.md)                             | IPC and backend contract index    |
| [`references/product/index.md`](./references/product/index.md)                                   | Product behavior specs            |
| [`references/generated/db-schema.md`](./references/generated/db-schema.md)                       | Generated SQLite schema summary   |
| [`plans/README.md`](./plans/README.md)                                                           | How to add executable plans       |
| [`archive/README.md`](./archive/README.md)                                                       | Completed and superseded material |

## Maintenance Rules

- Keep `docs/plans/` small. If a plan is complete, superseded, or only a
  candidate list, move it to `docs/archive/`.
- Put durable behavior and API facts in `docs/references/`, not in plans or the
  changelog.
- Update `CHANGELOG.md` for completed user-visible or maintainer-visible
  changes. Do not backfill future intentions there.
- Keep future ideas in the GitHub Project unless they are being turned into an
  executable plan.
- Generated docs must name their generator and output path in `scripts/README.md`.
