# ADR 0013 — Use standards as product quality baselines

Date: 2026-07-28
Status: accepted

## Context

OpenKara is a cross-platform desktop app. It uses a WebView, native window
features, local audio, remote libraries, OAuth, and signed updates. The
project has quality checks for many of these areas. It needs a common source
for the quality target of each area.

## Decision

OpenKara uses versioned standards as product quality baselines. The active
standards, their scope, and their evidence are in
[`docs/references/product-standards.md`](../references/product-standards.md).

WCAG 2.2 level AA is the accessibility target for all rendered UI. WCAG2ICT
extends that target to desktop software behavior. WAI-ARIA Authoring Practices
define custom widget behavior. ISO 9241 defines the human-centred design and
interaction review model. ISO/IEC 25010 defines the product quality model.

The standards profile also defines product copy, locale tags, external
protocols, security, release supply chain, and audio measurement. A change
uses every profile that applies to its affected surface. A new standard
version, a changed conformance target, or a removed profile needs a new ADR.

Automated checks provide repeatable evidence. A complete conformance claim
also needs review of the affected user process. An exception records the
standard clause, user effect, reason, and compensating control in the change
that introduces it.

## Consequences

- Every interactive feature has a keyboard path, accessible name, appropriate
  role, state or value, focus order, and status feedback where needed.
- Product changes use the ISO 9241 interaction principles and the ISO/IEC
  25010 quality model during acceptance review.
- User-visible text, locale data, network protocols, release artifacts, and
  audio measurements use their selected standards instead of ad hoc formats.
- New standard-specific test code names the exact standard version and clause
  family that it checks.
- A passing automation suite is evidence for the covered paths. Release
  conformance claims include the remaining manual and platform checks.
