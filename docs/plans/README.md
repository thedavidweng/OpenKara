# Plan

This folder holds the **active engineering plan** and **tech-debt tracker** only. Everything else under `docs/` is either design reference, product behavior specs, frozen contracts, generated summaries, or **historical** material under `docs/archive/`.

| File                                                         | Purpose                                                                                          |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| [plan.md](./plan.md)                                         | **Single active plan skeleton** — candidate streams for the next slice                           |
| [native-feel-optimization.md](./native-feel-optimization.md) | Native feel optimization — cursor, startup flicker, popover edge handling, window resize (P0-P3) |
| [f1-frontend-completion.md](./f1-frontend-completion.md)     | Completed F1 frontend completion plan and acceptance checklist; archive with this plan cycle     |
| [tech-debt.md](./tech-debt.md)                               | Cross-cutting debt that does not belong to one feature line                                      |

**Rules**

- When `plan.md` is finished, move it to [`../archive/plans/`](../archive/plans/) (or add an outcome appendix there) and write a new `plan.md` for the next slice.
- Older point-in-time plans and priority snapshots already live under [`../archive/plans/`](../archive/plans/).
- Completed implementation plans (like `f1-frontend-completion.md`) should be archived after the planning cycle ends.

Historical agreed priority tables (no task breakdown): [`../archive/plans/future-work-and-hardening-priorities-2026-05.md`](../archive/plans/future-work-and-hardening-priorities-2026-05.md).
