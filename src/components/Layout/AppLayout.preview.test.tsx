// @vitest-environment jsdom

import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, fireEvent, cleanup } from "@testing-library/react";
import type { WindowShellState } from "@/lib/window-shell";

const {
  mockLayoutState,
  mockLibraryState,
  mockSettingsState,
  mockQueueState,
  mockPlayerState,
  mockPlaylistState,
  mockRotationState,
} = vi.hoisted(() => ({
  mockLayoutState: {
    sidebarVisible: true,
    sidebarWidth: 260,
    setSidebarWidth: vi.fn(),
    toggleSidebar: vi.fn(),
  },
  mockLibraryState: {
    songs: [],
    filter: "all" as const,
    setFilter: vi.fn(),
    separationStatuses: {},
    batchSeparation: null,
    importFiles: vi.fn(),
  },
  mockSettingsState: {
    isOpen: false,
    open: vi.fn(),
    toggle: vi.fn(),
  },
  mockQueueState: {
    isOpen: false,
    toggle: vi.fn(),
  },
  mockPlayerState: {
    airPlayOutput: { active: false, audioActive: false, phase: "idle" },
  },
  mockPlaylistState: {
    playlists: [],
    activePlaylistId: null,
    loadPlaylists: vi.fn(),
    createPlaylist: vi.fn(),
    setActivePlaylist: vi.fn(),
  },
  mockRotationState: {
    loadRotation: vi.fn(),
  },
}));

vi.mock("@/components/Bootstrap/ModelBootstrapBanner", () => ({
  ModelBootstrapBanner: () => <div data-testid="model-banner" />,
}));

vi.mock("@/components/Layout/GlobalProgressBar", () => ({
  GlobalProgressBar: () => <div data-testid="global-progress" />,
}));

vi.mock("@/components/Library/ImportCdgChoiceDialog", () => ({
  ImportCdgChoiceDialog: () => <div data-testid="import-cdg-dialog" />,
}));

vi.mock("@/components/Player/PlaybackBar", () => ({
  PlaybackBar: () => <div data-testid="playback-bar" />,
}));

vi.mock("@/components/Playback/PlaybackStage", () => ({
  PlaybackStage: ({ presentation = "standard" }: { presentation?: string }) => (
    <div data-testid="playback-stage" data-presentation={presentation} />
  ),
}));

vi.mock("@/components/Player/QueuePanel", () => ({
  QueuePanel: () => <div data-testid="queue-panel" />,
}));

vi.mock("@/components/Settings/SettingsOverlay", () => ({
  SettingsOverlay: () => <div data-testid="settings-overlay" />,
}));

vi.mock("./Sidebar", () => ({
  Sidebar: ({ previewMode }: { previewMode?: boolean }) => (
    <div data-testid="sidebar" data-preview={previewMode ?? false} />
  ),
}));

vi.mock("./SidebarRail", () => ({
  SidebarRail: ({
    children,
    resizable,
  }: {
    children: React.ReactNode;
    resizable?: boolean;
  }) => (
    <div data-testid="sidebar-rail" data-resizable={resizable ?? true}>
      {children}
    </div>
  ),
}));

vi.mock("./ToastContainer", () => ({
  ToastContainer: () => <div data-testid="toast-container" />,
}));

vi.mock("./WindowChrome", () => ({
  WindowChrome: ({
    previewMode,
    onImportMenuAction,
  }: {
    previewMode?: boolean;
    onImportMenuAction?: () => void;
    shellState: { tier: string; trafficLightInsetLeading: number };
  }) => (
    <div data-testid="window-chrome" data-preview={previewMode ?? false}>
      <button
        data-testid="import-action"
        onClick={onImportMenuAction}
        type="button"
      >
        Import
      </button>
    </div>
  ),
}));

vi.mock("@/hooks/use-animated-presence", () => ({
  useAnimatedPresence: () => ({
    shouldRender: false,
    className: "",
    onAnimationEnd: vi.fn(),
  }),
}));

vi.mock("@/lib/app-shortcuts", () => ({
  getShortcutPlatform: () => "mac",
  getShortcutDisplay: () => "",
  APP_SHORTCUTS: {
    toggleSettings: { id: "settings.toggle" },
  },
}));

vi.mock("@/lib/window-chrome", () => ({
  getWindowChromeVariant: () => "mac",
}));

const mockPromptImportFiles = vi.fn();
vi.mock("@/runtime/menu-runtime", () => ({
  promptImportFiles: (...args: unknown[]) => mockPromptImportFiles(...args),
}));

vi.mock("@/stores/layout-store", () => ({
  useLayoutStore: (selector: (state: typeof mockLayoutState) => unknown) =>
    selector(mockLayoutState),
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: (selector: (state: typeof mockLibraryState) => unknown) =>
    selector(mockLibraryState),
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: (selector: (state: typeof mockPlayerState) => unknown) =>
    selector(mockPlayerState),
}));

vi.mock("@/stores/queue-store", () => ({
  useQueueStore: (selector: (state: typeof mockQueueState) => unknown) =>
    selector(mockQueueState),
}));

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: (selector: (state: typeof mockSettingsState) => unknown) =>
    selector(mockSettingsState),
}));

vi.mock("@/stores/playlist-store", () => ({
  usePlaylistStore: (selector: (state: typeof mockPlaylistState) => unknown) =>
    selector(mockPlaylistState),
}));

vi.mock("@/stores/rotation-store", () => ({
  useRotationStore: (selector: (state: typeof mockRotationState) => unknown) =>
    selector(mockRotationState),
}));

const macShellState = {
  chromeVariant: "mac",
  tier: "mac",
  toolbarHeight: 48,
  trafficLightInsetLeading: 78,
  sidebarHeaderHeight: 28,
  sidebarWidth: 260,
} satisfies WindowShellState;

import { AppLayout } from "./AppLayout";

describe("AppLayout preview mode", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("does not call promptImportFiles when import is triggered in preview mode", () => {
    const { getAllByTestId } = render(
      <AppLayout initialWindowShellState={macShellState} previewMode />,
    );

    fireEvent.click(getAllByTestId("import-action")[0]);
    expect(mockPromptImportFiles).not.toHaveBeenCalled();
  });

  it("calls promptImportFiles when import is triggered in normal mode", () => {
    const { getAllByTestId } = render(
      <AppLayout initialWindowShellState={macShellState} />,
    );

    fireEvent.click(getAllByTestId("import-action")[0]);
    expect(mockPromptImportFiles).toHaveBeenCalledWith({
      importFiles: mockLibraryState.importFiles,
    });
  });

  it("does not call loadRotation in preview mode", () => {
    render(<AppLayout initialWindowShellState={macShellState} previewMode />);
    expect(mockRotationState.loadRotation).not.toHaveBeenCalled();
  });

  it("calls loadRotation in normal mode", () => {
    render(<AppLayout initialWindowShellState={macShellState} />);
    expect(mockRotationState.loadRotation).toHaveBeenCalled();
  });

  it("blocks click interactions on non-playlist targets in preview mode", () => {
    const onClick = vi.fn();
    const { container } = render(
      <AppLayout initialWindowShellState={macShellState} previewMode />,
    );

    const outer = container.firstElementChild as HTMLElement;
    const child = document.createElement("div");
    child.dataset.testid = "click-child";
    child.addEventListener("click", onClick);
    outer.appendChild(child);

    fireEvent.click(child);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("allows click interactions on preview-playlist targets in preview mode", () => {
    const onClick = vi.fn();
    const { container } = render(
      <AppLayout initialWindowShellState={macShellState} previewMode />,
    );

    const outer = container.firstElementChild as HTMLElement;
    const playlistChild = document.createElement("div");
    playlistChild.setAttribute("data-preview-playlist-switch", "true");
    playlistChild.addEventListener("click", onClick);
    outer.appendChild(playlistChild);

    fireEvent.click(playlistChild);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("allows click interactions on lyrics-interactive targets in preview mode", () => {
    const onClick = vi.fn();
    const { container } = render(
      <AppLayout initialWindowShellState={macShellState} previewMode />,
    );

    const outer = container.firstElementChild as HTMLElement;
    const lyricsButton = document.createElement("button");
    lyricsButton.setAttribute("data-preview-lyrics-interactive", "true");
    lyricsButton.addEventListener("click", onClick);
    outer.appendChild(lyricsButton);

    fireEvent.click(lyricsButton);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("allows pointerdown on lyrics-interactive targets in preview mode", () => {
    const onPointerDown = vi.fn();
    const { container } = render(
      <AppLayout initialWindowShellState={macShellState} previewMode />,
    );

    const outer = container.firstElementChild as HTMLElement;
    const lyricsViewport = document.createElement("div");
    lyricsViewport.setAttribute("data-preview-lyrics-interactive", "true");
    lyricsViewport.addEventListener("pointerdown", onPointerDown);
    outer.appendChild(lyricsViewport);

    fireEvent.pointerDown(lyricsViewport);
    expect(onPointerDown).toHaveBeenCalledTimes(1);
  });

  it("allows click interactions on sidebar-toggle targets in preview mode", () => {
    const onClick = vi.fn();
    const { container } = render(
      <AppLayout initialWindowShellState={macShellState} previewMode />,
    );

    const outer = container.firstElementChild as HTMLElement;
    const sidebarToggle = document.createElement("button");
    sidebarToggle.setAttribute("data-preview-sidebar-toggle", "true");
    sidebarToggle.addEventListener("click", onClick);
    outer.appendChild(sidebarToggle);

    fireEvent.click(sidebarToggle);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("blocks context menu in preview mode", () => {
    const onContext = vi.fn();
    const { container } = render(
      <AppLayout initialWindowShellState={macShellState} previewMode />,
    );

    const outer = container.firstElementChild as HTMLElement;
    const child = document.createElement("div");
    child.addEventListener("contextmenu", onContext);
    outer.appendChild(child);

    fireEvent.contextMenu(child);
    expect(onContext).not.toHaveBeenCalled();
  });
});
