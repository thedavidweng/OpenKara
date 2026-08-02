# Issue 303 and PR 310 Closure Design

## Outcome

OpenKara must use one executable source for each release acceptance check. The
change must connect the existing acceptance tests to Nightly and Release. It
must also remove invalid remote playback states that PR 310 introduced.

## Scope

### Accessibility evidence

The tests in `tests/e2e/accessibility/` are the only web accessibility suite.
Nightly runs this directory with the extended matrix enabled. The old
`tests/e2e/accessibility.spec.ts` file is deleted.

Playwright selects the accessibility, smoke, and remaining UI suites. The
workflow does not discover files with PowerShell. Each file belongs to one
suite. This rule prevents duplicate and skipped tests.

### Windows desktop scenarios

The desktop driver owns its supported scenario names, actions, and report
labels. The driver does not load a second scenario description from JSON.
`tests/desktop/windows/scenarios.json` is deleted.

Reports keep the current stable scenario names and assertion identifiers. The
change does not replace the PowerShell driver with a new test framework.

### Nightly and release evidence

Nightly aggregation validates a fixed set of required evidence kinds. It fails
when a required kind is absent, an assertion fails, or evidence refers to a
different commit. It produces one machine-readable manifest for the Nightly
commit.

Release accepts Nightly evidence only when the evidence commit equals the
release candidate commit. Publish jobs remain blocked when required candidate
evidence is absent or invalid. Existing release evidence remains the source for
published asset identity and digests.

### Remote playback pruning

Remote stem materialization has one guarded operation. Callers that do not
need cancellation pass an always-current predicate. The operation uses one
domain error type.

The remote stem loader returns decoded base audio and required
`LoadedStems`. It does not return an optional stem value. The general playback
load result keeps `Option<LoadedStems>` because ordinary tracks do not have
stems.

Provider conformance and fault injection stay test-only. Frontend event
modules stay separate because they own domain subscriptions and state effects.
This change does not add a playback session abstraction or a new provider
layer.

## Error behavior

CI fails at the first contract boundary that can name the missing or invalid
evidence. Remote materialization returns the existing typed remote error.
Command and playback boundaries map that error once.

The implementation does not add fallback behavior. It removes paths that can
hide a missing required stem or missing release evidence.

## Acceptance evidence

- Workflow tests prove suite selection, required Nightly evidence, commit
  binding, and release gating.
- Desktop report tests prove that each supported scenario is executable without
  a second scenario registry.
- Rust regression tests prove current-request cancellation and required remote
  stem results.
- Full release-sensitive verification passes: frontend lint, frontend build,
  frontend tests, Rust tests, and Tauri build.

## Exclusions

This change does not add new product features, provider implementations, test
adapters, or platform automation frameworks. It does not claim that a browser
accessibility test replaces native UI Automation evidence.
