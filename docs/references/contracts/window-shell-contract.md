# Window Shell Contract

This contract documents the backend-facing shell snapshot. The shared React
layout uses it to adapt host chrome and window metrics. **Native macOS is a
host capability layer only** — there is no second product webview or
shell-specific React entry tree.

## Commands

- `get_window_shell_state() -> WindowShellStateSnapshot`
- `set_native_sidebar_visibility(visible: boolean) -> void` (legacy no-op; kept for IPC stability)
- `window_ready() -> void`

The frontend calls `window_ready` after the first real app screen commits.
The main window may start hidden. This command is the contract boundary. It
lets the host reveal the window without an empty WebView.

- `restart_app() -> void`

## Payload

```ts
type WindowShellChromeVariant = "desktop" | "mac";
type WindowShellTier = "desktop" | "mac";

interface WindowShellStateSnapshot {
  chrome_variant: WindowShellChromeVariant;
  tier: WindowShellTier;
  toolbar_height: number;
  traffic_light_inset_leading: number;
  sidebar_header_height: number;
  sidebar_width: number;
}
```

## Semantics

- `chrome_variant`
  - `desktop`: existing Windows/Linux custom titlebar path
  - `mac`: macOS shell metrics and chrome tokens
- `tier`
  - `desktop`: no mac-specific shell treatment
  - `mac`: macOS AppKit window chrome + traffic-light metrics with the **same** single-webview React tree as other tiers
- `toolbar_height`
  - Native toolbar/titlebar height mirrored into CSS tokens
- `traffic_light_inset_leading`
  - Leading inset reserved for standard window controls
- `sidebar_header_height`
  - Reserved strip height so toolbar and sidebar content can clear the system traffic lights
- `sidebar_width`
  - Shared shell width token for sidebar rail/layout alignment across all platforms

## Shared UI Boundary

- Library, playback, lyrics, settings, and layout components live in one shared React/Tailwind UI.
- `WindowShellStateSnapshot` only controls **window-level** metrics (toolbar height, traffic-light layouts, sidebar width token). It must not imply a second rendering path or split-webview product shell.
- Windows/Linux return `desktop` values unless their shell design changes intentionally.
- Native visual work happens in shared components and CSS tokens. The host tier only supplies metric tokens.

## Sidebar Visibility Sync

- `set_native_sidebar_visibility` is a **no-op** after single-webview unification. The frontend toggles sidebar layout inside the shared tree.

## Stability Rules

- Windows/Linux must return `desktop` values unless their shell design changes intentionally.
- macOS must use the shared single-webview UI path.
- Frontend code must treat unknown or missing values as a signal to fall back to existing desktop tokens. It must not invent a new shell mode.
