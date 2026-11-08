# ADR 0050 — Build controls from shadcn/ui on Base UI

Date: 2026-11-07
Status: accepted

## Context

Each OpenKara screen styled its own buttons, tabs, menus and inputs. The
results did not match, and hand-written keyboard and focus code had defects.
One example is the WebKit Tab regression in dialogs. OpenKara already has its
own theme tokens and a 13 px root font size. The shadcn/ui defaults assume a
16 px root and their own color names. Some of those names collide with
OpenKara tokens, for example `--color-accent` and `--color-border`.

## Decision

Use shadcn/ui components with Base UI primitives (`style: base-nova`). Copy the
components into `src/components/ui/` with the shadcn CLI (`pnpm dlx shadcn`).
Do not install the CLI as a dependency. Map the shadcn color names to OpenKara
tokens in one `@theme inline` block in `src/styles/globals.css`. `inline` keeps
those names out of `:root`, so `var(--color-accent)` still reads the OpenKara
value. Copy the Base UI state variants from `shadcn/tailwind.css` into the
same file. Edit the copied components to use the 13 px scale: `text-base`
instead of `text-sm`, and no control smaller than 24 px. The `dark:` variant
applies unless `data-theme="light"` is set.

## Consequences

- New controls start from `src/components/ui/`. Fix shared defects there.
- A component added later with the CLI needs the same text-size and
  target-size edits.
- Dialogs inside Preferences still use `useModalDialog`. Its document-level
  Tab and Escape handling conflicts with Base UI focus management. Move all
  dialogs to Base UI together, not one at a time.
- Button transitions exclude opacity, so a re-enabled button never shows a
  low-contrast frame.
