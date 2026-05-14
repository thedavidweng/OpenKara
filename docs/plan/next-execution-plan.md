# Next execution plan

> **Status:** Active · **Last updated:** 2026-05-14  
> **Supersedes:** Priority snapshot in [`../archive/plans/future-work-and-hardening-priorities-2026-05.md`](../archive/plans/future-work-and-hardening-priorities-2026-05.md) (tables kept for history only).

## Scope

Deliver **all agreed hardening streams** (lyrics → remote → separation → AirPlay/presentation → packaging → documentation ownership → generated DB schema doc) and **new capability 1 — saved playlists & singer rotation**, in the order below. Each stream may ship across one or more releases; the sequence defines **dependency and attention order**, not a single release gate.

References: [`../implementation-status.md`](../implementation-status.md), [`../design-docs/roadmap.md`](../design-docs/roadmap.md), [`../references/contracts/README.md`](../references/contracts/README.md), [`../exec-plans/tech-debt-tracker.md`](../exec-plans/tech-debt-tracker.md).

---

## H1 — Lyrics pipeline (hardening priority 1)

**Goal:** Reduce lyrics-related regressions and edge cases across sync, plain text, romanization, and language metadata.

**Work items (non-exhaustive):**

- Regression sweep: sync during long playback, seek, pause/resume; plain-text paging; offset + romanization overrides.
- Fuzz or fixture expansion for LRC edge cases aligned with [`../references/contracts/phase-4-lyrics-contract.md`](../references/contracts/phase-4-lyrics-contract.md).
- Confirm LRCLIB + LrcApi fallback behavior matches architecture docs when metadata is partial.

**Verification:** `pnpm format` → `pnpm lint` → `pnpm build` → `pnpm test`; touch Rust only if backend lyrics paths change → `cd src-tauri && cargo test -q`.

---

## H2 — Remote libraries (hardening priority 2)

**Goal:** Harden OAuth/WebDAV flows after v0.8.0 reauthorization unification — offline, token expiry, conflict recovery.

**Work items:**

- Provider matrix smoke: Drive / Dropbox / WebDAV — connect, idle, refresh, disconnect, reauthorize.
- User-visible errors: structured errors per [`../references/contracts/phase-5-error-contract.md`](../references/contracts/phase-5-error-contract.md); no token-bearing URLs in logs (CodeQL pattern).
- Cross-check with archived learnings [`../archive/plans/remote-library-hardening.md`](../archive/plans/remote-library-hardening.md) for any residual gaps.

**Verification:** Same as H1; add targeted integration tests where gaps are found.

---

## H3 — Separation runtime (hardening priority 3)

**Goal:** Stable ONNX session lifecycle, provider selection, and user-visible progress/errors on Windows/Linux/macOS.

**Work items:**

- Windows DirectML vs fallback paths; Linux XNNPACK/CPU; session cache keys per existing settings.
- Long-run separation: cancellation, resume/checkpoint invariants per [`../references/contracts/phase-3-separation-contract.md`](../references/contracts/phase-3-separation-contract.md).
- Memory / duration sanity on large files (document limits if any).

**Verification:** `cd src-tauri && cargo test -q`; manual smoke on at least two OS tiers if EP selection changes.

---

## H4 — AirPlay & presentation output (hardening priority 4)

**Goal:** Predictable audience / second-output behavior; CI-friendly tests.

**Work items:**

- Align implementation with [`../references/contracts/presentation-output-contract.md`](../references/contracts/presentation-output-contract.md) and playback contract where events intersect.
- Keep environment-sensitive tests explicitly gated or quarantined (see `AGENTS.md` Windows notes; Linux AirPlay-related skips).

**Verification:** Frontend + any Rust changes per AGENTS matrix.

---

## H5 — Packaging & supply chain (hardening priority 5)

**Goal:** Release automation stays green: WinGet/Flatpak manifest generation, permissions, artifact layout.

**Work items:**

- `packaging/winget/`, `packaging/flatpak/`, `.github/workflows/release.yml` / `packaging.yml` — validate generators on version bumps.
- Document any required secrets / fork compare URLs in [`../design-docs/releasing.md`](../design-docs/releasing.md).

**Verification:** `pnpm test` (includes packaging tests where applicable); dry-run release workflow when touching workflows.

---

## H6 — Documentation ownership (hardening priority 6)

**Goal:** User-visible behavior has a spec home; README/website/design docs do not diverge silently.

**Work items:**

- Extend [`../product-specs/`](../product-specs/) for high-churn areas touched in H1–H5 (incremental).
- Update [`../exec-plans/tech-debt-tracker.md`](../exec-plans/tech-debt-tracker.md) when items close.

**Verification:** `pnpm format`; spot-check links from [`../README.md`](../README.md).

---

## H7 — Generated DB schema doc (hardening priority 7)

**Goal:** `docs/generated/db-schema.md` tracks migrations without hand-editing drift.

**Work items:**

- Add a small script (e.g. under `scripts/`) that regenerates the doc from `src-tauri/migrations/`; wire optional `pnpm` script; document in `scripts/README.md` or [`../README.md`](../README.md).

**Verification:** Script is idempotent; CI or contributor docs mention when to run it.

---

## F1 — Saved playlists & singer rotation (new capability priority 1)

**Goal:** Named playlists, singer assignment, and turn-based queue workflows so multi-singer sessions do not rely on ad-hoc queue manipulation alone.

**Work items (high level — refine before implementation):**

- **Product:** Define minimal UX: create/rename/delete playlist; add/remove songs; assign “current singer”; optional rotation rules (round-robin, manual advance).
- **Data:** SQLite schema + migrations; portable paths; interaction with existing library/queue stores.
- **IPC / contracts:** Extend or add commands/events per [`../references/contracts/phase-1-library-contract.md`](../references/contracts/phase-1-library-contract.md) and playback contract as needed — **update contract docs in the same change** as IPC.
- **i18n:** New strings in `src/locales/`.

**Verification:** Full frontend + Rust matrix per `AGENTS.md` for cross-stack changes; add Vitest and Rust tests for persistence and queue rules.

**Out of scope for F1:** Mic capture, pitch shift, session recording, mobile companion (later priorities per archived table).

---

## Completion

When H1–H7 and F1 are satisfactorily delivered:

1. Archive this file under `docs/archive/plans/` with a dated filename (or add an “Outcome” appendix and move).
2. Replace `next-execution-plan.md` with the **next** single plan (e.g. capability 2 — mic — plus any new hardening discovered during execution).
