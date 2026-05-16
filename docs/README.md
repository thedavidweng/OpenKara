# Documentation

The public website source lives at [`../website/`](../website/) so this folder
can stay focused on engineering docs, product specs, references, generated
summaries, and historical records.

## Design docs

| Document                                                                                     | Description                                                       |
| -------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| [design-docs/index.md](./design-docs/index.md)                                               | Entry point for architecture, roadmap, release, and delivery docs |
| [design-docs/architecture.md](./design-docs/architecture.md)                                 | System architecture, tech stack, data flow, and runtime design    |
| [design-docs/core-beliefs.md](./design-docs/core-beliefs.md)                                 | Core product and engineering principles                           |
| [design-docs/project-structure.md](./design-docs/project-structure.md)                       | Directory layout and module responsibilities                      |
| [design-docs/roadmap.md](./design-docs/roadmap.md)                                           | Technical roadmap, API contracts, and risk notes                  |
| [design-docs/releasing.md](./design-docs/releasing.md)                                       | Release workflow, Homebrew distribution, and future channels      |
| [design-docs/performance/phase-5-baseline.md](./design-docs/performance/phase-5-baseline.md) | Backend benchmark baseline for profiling work                     |

## Plans

| Document                                                                 | Description                                                                                      |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| [plans/README.md](./plans/README.md)                                     | What lives in `docs/plans/` and how files age out                                                |
| [plans/plan.md](./plans/plan.md)                                         | **Active plan** — hardening H1–H8 + capability F1 (playlists / singer rotation) until superseded |
| [plans/native-feel-optimization.md](./plans/native-feel-optimization.md) | Native feel optimization — cursor, startup flicker, popover edge handling, window resize         |
| [plans/tech-debt.md](./plans/tech-debt.md)                               | Cross-cutting debt and housekeeping                                                              |

Older point-in-time plans live under [archive/plans/](archive/README.md).

## Generated docs

| Document                                           | Description                                           |
| -------------------------------------------------- | ----------------------------------------------------- |
| [generated/db-schema.md](./generated/db-schema.md) | Current SQLite schema summary derived from migrations |

## Product specs

| Document                                                                       | Description                                     |
| ------------------------------------------------------------------------------ | ----------------------------------------------- |
| [product-specs/index.md](./product-specs/index.md)                             | Product-spec index and ownership guidance       |
| [product-specs/new-user-onboarding.md](./product-specs/new-user-onboarding.md) | First-run and first-import experience reference |

## References

| Document                                                                                                               | Description                                                    |
| ---------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| [references/index.md](./references/index.md)                                                                           | Reference-doc index                                            |
| [references/contracts/README.md](./references/contracts/README.md)                                                     | Frozen backend contract index                                  |
| [references/contracts/phase-6-model-bootstrap-contract.md](./references/contracts/phase-6-model-bootstrap-contract.md) | Runtime model bootstrap contract for current distribution work |

## Archive

| Document                                 | Description                                                                                      |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------ |
| [archive/README.md](./archive/README.md) | Historical plans and snapshots; browse [`archive/plans/`](./archive/plans/) for individual files |
