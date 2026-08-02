import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { APP_SHORTCUTS, getShortcutDisplay } from "@/lib/app-shortcuts";
import type { WindowShellState } from "@/lib/window-shell";
import { Toolbar } from "./Toolbar";

const macShellState = {
  chromeVariant: "mac",
  tier: "mac",
  toolbarHeight: 48,
  trafficLightInsetLeading: 78,
  sidebarHeaderHeight: 28,
  sidebarWidth: 260,
} satisfies WindowShellState;

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string) => key,
    }),
  };
});

vi.mock("@/components/Library/ImportButton", () => ({
  ImportButton: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock("@/components/Overlay/Tooltip", () => ({
  Tooltip: ({
    children,
    label,
    shortcut,
  }: {
    children: React.ReactNode;
    label: string;
    shortcut?: string;
  }) => (
    <span data-tooltip-label={label} data-tooltip-shortcut={shortcut}>
      {children}
    </span>
  ),
}));

vi.mock("@/components/Player/AirPlayRouteButton", () => ({
  AirPlayRouteButton: ({ previewMode }: { previewMode?: boolean }) => (
    <div
      data-airplay-button="true"
      data-airplay-preview={previewMode ? "true" : undefined}
    />
  ),
}));

describe("Toolbar drag region", () => {
  test("keeps the toolbar root interactive and isolates drag affordances", () => {
    const markup = renderToStaticMarkup(
      <Toolbar
        onToggleSidebar={() => {}}
        onToggleSettings={() => {}}
        settingsOpen={false}
        sidebarVisible={false}
      />,
    );

    expect(markup).not.toContain(
      'bg-[color-mix(in_srgb,var(--color-toolbar)_92%,transparent)] px-4 shadow-[0_1px_0_rgba(255,255,255,0.02)] backdrop-blur-xl" data-tauri-drag-region',
    );
    expect(markup).toContain("data-tauri-drag-region");
  });

  test("does not insert a sidebar-hidden spacer before the toggle button", () => {
    const markup = renderToStaticMarkup(
      <Toolbar
        onToggleSidebar={() => {}}
        onToggleSettings={() => {}}
        settingsOpen={false}
        sidebarVisible={false}
      />,
    );

    expect(markup).not.toContain("w-[54px]");
  });

  test("shows import tooltip metadata with the shared shortcut", () => {
    const markup = renderToStaticMarkup(
      <Toolbar
        onToggleSidebar={() => {}}
        onToggleSettings={() => {}}
        settingsOpen={false}
        sidebarVisible={true}
      />,
    );

    expect(markup).toContain('data-tooltip-label="toolbar.import"');
    expect(markup).toContain(
      `data-tooltip-shortcut="${getShortcutDisplay(APP_SHORTCUTS.importFiles)}"`,
    );
  });

  test("renders separate AirPlay and monitor controls on macOS", () => {
    const markup = renderToStaticMarkup(
      <Toolbar
        onToggleSidebar={() => {}}
        onToggleSettings={() => {}}
        shellState={macShellState}
        settingsOpen={false}
        sidebarVisible
      />,
    );

    expect(markup).toContain('data-airplay-button="true"');
    expect(markup).toContain('aria-label="player.selectMonitor"');
  });

  test("exposes the mac shell tier and traffic-light spacing through markup", () => {
    const markup = renderToStaticMarkup(
      <Toolbar
        onToggleSidebar={() => {}}
        onToggleSettings={() => {}}
        shellState={macShellState}
        settingsOpen={false}
        sidebarVisible
      />,
    );

    expect(markup).toContain('data-window-shell-tier="mac"');
    expect(markup).toContain("--window-shell-leading-controls-space:78px");
  });

  test("adds browser-only macOS traffic lights and AirPlay fallback to the preview", () => {
    const markup = renderToStaticMarkup(
      <Toolbar
        onToggleSidebar={() => {}}
        onToggleSettings={() => {}}
        previewMode
        shellState={macShellState}
        settingsOpen={false}
        sidebarVisible
      />,
    );

    expect(markup).toContain('data-preview-traffic-lights="true"');
    expect(markup).toContain('data-airplay-preview="true"');
    expect(markup).toContain('data-preview-sidebar-toggle="true"');
  });

  test("can omit the leading sidebar/import controls for native split shells", () => {
    const markup = renderToStaticMarkup(
      <Toolbar
        hideLeadingShellControls
        onToggleSidebar={() => {}}
        onToggleSettings={() => {}}
        shellState={macShellState}
        settingsOpen={false}
        sidebarVisible
      />,
    );

    expect(markup).not.toContain('aria-label="toolbar.toggleSidebar"');
    expect(markup).not.toContain('aria-label="toolbar.import"');
  });
});
