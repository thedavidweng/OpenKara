# Future work & hardening (archived snapshot)

> **Archived:** 2026-05-14 — Superseded by the single active execution plan in [`../../planning/plan.md`](../../planning/plan.md).  
> This file preserves the agreed **priority ordering** as of 2026-05-14; do not edit here.

## How to use this doc

1. **Shipped facts** live in [`../../implementation-status.md`](../../implementation-status.md) and version-tagged releases — not here.
2. **Priority** below was **agreed** as of 2026-05-14 (maintainer confirmation).
3. **Hardening** items may ship in any patch/minor release; they do not need their own marketing version.

## New capabilities (v0.9+)

| Theme                             | User outcome                                          | Dependencies / risk                                       | Priority |
| --------------------------------- | ----------------------------------------------------- | --------------------------------------------------------- | -------- |
| Saved playlists & singer rotation | Host multi-singer sessions without ad-hoc queue hacks | UX design, persistence model                              | **1**    |
| Mic input & vocal effects         | Sing along with monitoring, simple FX                 | Audio I/O latency, feedback control, cross-platform QA    | **2**    |
| Pitch & key shift                 | Match singer range on the fly                         | Real-time DSP quality vs CPU; integration with stem mixer | **3**    |
| Session recording                 | Capture performance to file                           | Legal/UX copy, storage paths, sync with stems             | **4**    |
| Mobile companion                  | Remote control / lyrics on a second device            | Transport security, discovery, scope                      | **5**    |

Lower **Priority** number = ship first. **1** is intentionally closest to existing queue/library work before taking on heavy DSP or a second client.

## Hardening & optimization (existing product)

These areas already ship to users; **1** is the highest leverage before stacking major new features.

| Area                              | Why it matters                       | Evidence / notes                                                                                                               | Priority |
| --------------------------------- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | -------- |
| **Lyrics pipeline**               | Core karaoke UX                      | Recent fixes: sync stall, plain-text layout, romanization language overrides — still high churn surface                        | **1**    |
| **Remote libraries**              | Complex OAuth/WebDAV edge cases      | Reauthorization unified in v0.8.0; residual provider quirks and offline behavior                                               | **2**    |
| **Separation runtime**            | Long CPU jobs, platform-specific EPs | ONNX provider selection, fallbacks, and user-visible errors already iterated in v0.5+ — watch for regressions on Windows/Linux | **3**    |
| **AirPlay / presentation output** | Platform-specific AV behavior        | CI has historically treated some playback tests as environment-sensitive (e.g. Linux)                                          | **4**    |
| **Packaging & supply chain**      | Release friction affects trust       | Flatpak/WinGet paths improved through v0.8.1; keep manifest generators and CI in sync                                          | **5**    |
| **Documentation ownership**       | Prevents spec drift                  | See [`../../planning/tech-debt.md`](../../planning/tech-debt.md)                                                               | **6**    |
| **Generated schema doc**          | Onboarding for DB changes            | `docs/generated/db-schema.md` is manual today                                                                                  | **7**    |

## Priority decision log

| Date       | Decision                                                                                                  | Outcome                                                                                                                                                                                                     |
| ---------- | --------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 2026-05-13 | Initial backlog seeded from `implementation-status.md` + `tech-debt-tracker.md` + post–v0.7.0 git history | All priorities **TBD** pending maintainer/product review                                                                                                                                                    |
| 2026-05-14 | Agreed ordering (maintainer)                                                                              | **Hardening:** lyrics → remote → separation → AirPlay/presentation → packaging → docs ownership → `db-schema` automation. **v0.9+ features:** playlists/singer rotation → mic → pitch → recording → mobile. |

## Related links

- [`../../implementation-status.md`](../../implementation-status.md) — released milestones
- [`../../design-docs/roadmap.md`](../../design-docs/roadmap.md) — technical contracts and stack risks
- [`../../planning/tech-debt.md`](../../planning/tech-debt.md) — cross-cutting debt items
