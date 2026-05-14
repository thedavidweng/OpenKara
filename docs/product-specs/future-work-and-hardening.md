# Future work & hardening

> **Last updated:** 2026-05-13  
> **Purpose:** Single place to align on **what to build next** (new capabilities) versus **what to harden** (reliability, performance, maintainability). This doc is meant to change often; [`implementation-status.md`](../implementation-status.md) remains the canonical shipped-milestone list.

## How to use this doc

1. **Shipped facts** live in [`implementation-status.md`](../implementation-status.md) and version-tagged releases — not here.
2. **Priority** is expressed as _proposed_ until maintainers tick a decision row in [Priority decision log](#priority-decision-log) (or replace that section with dated ADRs).
3. **Hardening** items may ship in any patch/minor release; they do not need their own marketing version.

## Proposed new capabilities (v0.9+)

| Theme                             | User outcome                                          | Dependencies / risk                                       | Proposed priority |
| --------------------------------- | ----------------------------------------------------- | --------------------------------------------------------- | ----------------- |
| Mic input & vocal effects         | Sing along with monitoring, simple FX                 | Audio I/O latency, feedback control, cross-platform QA    | TBD               |
| Saved playlists & singer rotation | Host multi-singer sessions without ad-hoc queue hacks | UX design, persistence model                              | TBD               |
| Pitch & key shift                 | Match singer range on the fly                         | Real-time DSP quality vs CPU; integration with stem mixer | TBD               |
| Session recording                 | Capture performance to file                           | Legal/UX copy, storage paths, sync with stems             | TBD               |
| Mobile companion                  | Remote control / lyrics on a second device            | Transport security, discovery, scope                      | TBD               |

## Proposed hardening & optimization (existing product)

These areas already ship to users; investment here reduces incidents and support load before stacking new features.

| Area                              | Why it matters                       | Evidence / notes                                                                                                               | Proposed priority |
| --------------------------------- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ----------------- |
| **Lyrics pipeline**               | Core karaoke UX                      | Recent fixes: sync stall, plain-text layout, romanization language overrides — still high churn surface                        | TBD               |
| **Remote libraries**              | Complex OAuth/WebDAV edge cases      | Reauthorization unified in v0.8.0; residual provider quirks and offline behavior                                               | TBD               |
| **Separation runtime**            | Long CPU jobs, platform-specific EPs | ONNX provider selection, fallbacks, and user-visible errors already iterated in v0.5+ — watch for regressions on Windows/Linux | TBD               |
| **AirPlay / presentation output** | Platform-specific AV behavior        | CI has historically treated some playback tests as environment-sensitive (e.g. Linux)                                          | TBD               |
| **Packaging & supply chain**      | Release friction affects trust       | Flatpak/WinGet paths improved through v0.8.1; keep manifest generators and CI in sync                                          | TBD               |
| **Documentation ownership**       | Prevents spec drift                  | See [`tech-debt-tracker.md`](../exec-plans/tech-debt-tracker.md)                                                               | TBD               |
| **Generated schema doc**          | Onboarding for DB changes            | `docs/generated/db-schema.md` is manual today                                                                                  | TBD               |

## Priority decision log

_Use this table to record agreed ordering. Edit in place; prefer dated rows over rewriting history._

| Date       | Decision                                                                                                  | Outcome                                                  |
| ---------- | --------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| 2026-05-13 | Initial backlog seeded from `implementation-status.md` + `tech-debt-tracker.md` + post–v0.7.0 git history | All priorities **TBD** pending maintainer/product review |

## Related links

- [`../implementation-status.md`](../implementation-status.md) — released milestones
- [`../design-docs/roadmap.md`](../design-docs/roadmap.md) — technical contracts and stack risks
- [`../exec-plans/tech-debt-tracker.md`](../exec-plans/tech-debt-tracker.md) — cross-cutting debt items
- [`../exec-plans/active/index.md`](../exec-plans/active/index.md) — in-flight execution plans (empty when nothing is active)
