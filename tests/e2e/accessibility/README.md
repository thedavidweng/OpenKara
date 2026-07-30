# Accessibility tests

This group contains Playwright end-to-end tests for the accessibility of the OpenKara user interface. The tests use the `accessibility-test.ts` fixture, which extends the base test with helpers for themes, motion, forced colors, zoom, live-region monitoring, and axe-core scans.

The group is a scaffold. Detailed assertions that are not yet implemented are marked with `test.fixme` and a TODO.

## Specs

- `app-shell.spec.ts` — window chrome, toolbar, and main landmarks
- `library.spec.ts` — song list, search, sort, and batch separation controls
- `player.spec.ts` — playback bar, transport, seek, and volume controls
- `settings.spec.ts` — preferences dialog, sections, and form controls
- `separation.spec.ts` — stem separation progress and status
- `queue.spec.ts` — queue panel, reorder, and empty state
- `singer-rotation.spec.ts` — singer tags, assignment, and shuffle
- `remote-libraries.spec.ts` — remote repository wizard and settings sections
- `fullscreen.spec.ts` — monitor picker and fullscreen player view
- `errors.spec.ts` — error boundaries, alerts, and retry affordances
- `focus-order.spec.ts` — keyboard focus order, traps, and restoration
- `live-region.spec.ts` — aria-live announcements and status updates
- `themes-and-motion.spec.ts` — dark/light/reduced-motion/forced-colors/zoom matrices

## Run

```bash
pnpm test:a11y
```

To run a single spec:

```bash
pnpm test:a11y -- app-shell
```
