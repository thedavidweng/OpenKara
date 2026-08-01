# Accessibility tests

Playwright end-to-end accessibility coverage for the OpenKara WebView UI.

These tests are mandatory CI evidence for WCAG 2.2 A/AA. They run axe-core
scans and keyboard focus assertions against the mocked IPC app shell. Native
Windows UI Automation tree validation is a separate gate and is not covered
here.

## Fixtures

`fixtures/accessibility-test.ts` extends the base E2E fixture with:

- theme switching (`dark` / `light`) with settled CSS variable waits
- reduced-motion and forced-colors media emulation
- zoom scaling for reflow checks
- live-region announcement monitoring
- axe-core WCAG 2.2 A/AA scans that fail on any violation
- transition disabling so contrast checks do not race theme animations

## Specs

- `app-shell.spec.ts` — window chrome, toolbar, and main landmarks
- `library.spec.ts` — song list, search, sort, and alphabet rail
- `player.spec.ts` — playback bar, transport, seek, and volume controls
- `settings.spec.ts` — preferences dialog, sections, focus trap, labels
- `separation.spec.ts` — stem separation progress and status
- `queue.spec.ts` — queue panel, empty state, and drag instructions
- `singer-rotation.spec.ts` — singer tags and assignment controls
- `remote-libraries.spec.ts` — remote repository wizard and form labels
- `fullscreen.spec.ts` — presentation mode and fullscreen controls
- `errors.spec.ts` — error toasts and error-free shell load
- `focus-order.spec.ts` — keyboard focus order, traps, and restoration
- `live-region.spec.ts` — aria-live announcements and status banners
- `themes-and-motion.spec.ts` — dark/light × motion/zoom matrices
- `focus-visible.spec.ts` — keyboard focus indicators (screenshots opt-in)

## Run

Default suite (required CI path):

```bash
pnpm test:a11y
```

Expanded matrix (400% zoom, forced-colors, locale expansion):

```bash
pnpm test:a11y:matrix
```

Single spec:

```bash
pnpm test:a11y -- app-shell
```

Optional focus-visible screenshot baselines:

```bash
OKA_A11Y_SCREENSHOTS=1 pnpm test:a11y -- focus-visible
```

## Matrix policy

| Mode                                  | Coverage                                                         |
| ------------------------------------- | ---------------------------------------------------------------- |
| Default (`pnpm test:a11y`)            | dark/light axe on principal states; reduced-motion; 200% zoom    |
| Matrix (`OKA_ACCESSIBILITY_MATRIX=1`) | default coverage plus forced-colors, 400% zoom, locale expansion |

Every principal UI state runs axe in dark and light themes with visual-only
transitions disabled. Keyboard traps and focus restoration are asserted for
modal surfaces that the mocked UI can open.
