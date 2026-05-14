# Plan

This folder holds the **active engineering plan** and **tech-debt tracker** only. Everything else under `docs/` is either design reference, product behavior specs, frozen contracts, generated summaries, or **historical** material under `docs/archive/`.

| File                           | Purpose                                                                                         |
| ------------------------------ | ----------------------------------------------------------------------------------------------- |
| [plan.md](./plan.md)           | **Single active plan** — agreed hardening (H1–H7) + next capability (F1) until done or replaced |
| [tech-debt.md](./tech-debt.md) | Cross-cutting debt that does not belong to one feature line                                     |

**Rules**

- When `plan.md` is finished, move it to [`../archive/plans/`](../archive/plans/) (or add an outcome appendix there) and write a new `plan.md` for the next slice.
- Older point-in-time plans and priority snapshots already live under [`../archive/plans/`](../archive/plans/).

Historical agreed priority tables (no task breakdown): [`../archive/plans/future-work-and-hardening-priorities-2026-05.md`](../archive/plans/future-work-and-hardening-priorities-2026-05.md).
