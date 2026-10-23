// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { BackendProvider } from "@/lib/backend";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { Sidebar } from "./Sidebar";
import type { SourceSnapshot } from "@/types/ipc";

const setFilter = vi.fn();
const setActivePlaylist = vi.fn();
const setCatalogView = vi.fn();
const setYoutubeLinkOpen = vi.fn();
const settingsState = {
  hideBatchSeparate: true,
  hideUpgradeAll: true,
  stemMode: "two_stem",
  youtubeSourceEnabled: true,
};
const catalogState: {
  activeView: string;
  activeSourceId: string | null;
  setActiveSource: typeof setCatalogView;
  setActiveView: typeof setCatalogView;
  sources: SourceSnapshot[];
  sourceStates: Record<
    string,
    { session: { signed_in: boolean; expired: boolean } | null }
  >;
  youtubeLinkOpen: boolean;
  setYoutubeLinkOpen: typeof setYoutubeLinkOpen;
} = {
  activeView: "library",
  activeSourceId: null,
  setActiveSource: setCatalogView,
  setActiveView: setCatalogView,
  sources: [],
  sourceStates: {},
  youtubeLinkOpen: false,
  setYoutubeLinkOpen,
};

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: (
    selector: (state: {
      songs: never[];
      filter: "all";
      setFilter: typeof setFilter;
      separationStatuses: Record<string, never>;
      batchSeparation: null;
    }) => unknown,
  ) =>
    selector({
      songs: [],
      filter: "all",
      setFilter,
      separationStatuses: {},
      batchSeparation: null,
    }),
}));

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: (selector: (state: typeof settingsState) => unknown) =>
    selector(settingsState),
}));

vi.mock("@/stores/catalog-store", () => ({
  useCatalogStore: (selector: (state: typeof catalogState) => unknown) =>
    selector(catalogState),
}));

vi.mock("@/stores/playlist-store", () => ({
  usePlaylistStore: (
    selector: (state: {
      playlists: never[];
      activePlaylistId: null;
      isLoading: boolean;
      loadPlaylists: () => void;
      createPlaylist: () => void;
      setActivePlaylist: typeof setActivePlaylist;
    }) => unknown,
  ) =>
    selector({
      playlists: [],
      activePlaylistId: null,
      isLoading: false,
      loadPlaylists: () => {},
      createPlaylist: () => {},
      setActivePlaylist,
    }),
}));

vi.mock("@/components/Library/SearchBox", () => ({
  SearchBox: () => null,
}));
vi.mock("@/components/Library/SongList", () => ({
  SongList: () => null,
}));
vi.mock("@/components/Library/ImportButton", () => ({
  ImportButton: ({ children }: { children: React.ReactNode }) => children,
}));
vi.mock("@/components/Library/SortModeSelector", () => ({
  SortModeSelector: () => null,
}));
vi.mock("@/components/Settings/ConfirmationDialog", () => ({
  ConfirmationDialog: () => null,
}));
vi.mock("@/lib/errors", () => ({ notifyError: vi.fn() }));

function rowByLabel(container: HTMLElement, label: string) {
  const row = Array.from(container.querySelectorAll("button")).find((button) =>
    button.textContent?.includes(label),
  );
  if (!row) throw new Error(`missing sidebar row ${label}`);
  return row;
}

async function renderSidebar(root: ReturnType<typeof createRoot>) {
  await act(async () => {
    root.render(
      <BackendProvider backend={createMockBackend()}>
        <Sidebar />
      </BackendProvider>,
    );
  });
}

function rowByText(text: string): HTMLButtonElement | undefined {
  return Array.from(document.querySelectorAll("button")).find((b) =>
    b.textContent?.includes(text),
  );
}

function sourceRow(): HTMLButtonElement {
  return rowByText("Alpha") as HTMLButtonElement;
}

describe("Sidebar catalog source rows", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    setFilter.mockReset();
    setActivePlaylist.mockReset();
    setCatalogView.mockReset();
    setYoutubeLinkOpen.mockReset();
    catalogState.activeView = "library";
    catalogState.activeSourceId = null;
    catalogState.sourceStates = { alpha: { session: null } };
    catalogState.sources = [
      {
        source_id: "alpha",
        name: "Alpha",
        installed_version: "1",
        enabled: true,
        runtime_ready: true,
        state: "ready",
      } as SourceSnapshot,
    ];
    catalogState.youtubeLinkOpen = false;
    settingsState.youtubeSourceEnabled = true;
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  test("All Tracks, Separated, and source switch the catalog view", async () => {
    const backend = createMockBackend();
    await act(async () => {
      root.render(
        <BackendProvider backend={backend}>
          <Sidebar />
        </BackendProvider>,
      );
    });
    rowByLabel(container, "sidebar.allTracks").click();
    rowByLabel(container, "sidebar.separated").click();
    sourceRow().click();
    expect(setCatalogView).toHaveBeenCalledWith("library");
    expect(setCatalogView).toHaveBeenCalledWith("alpha");
    expect(setActivePlaylist).toHaveBeenCalledWith(null);
  });

  test("shows the online sources header with sign-in hint when signed out", async () => {
    await renderSidebar(root);
    expect(container.textContent).toContain("sidebar.onlineSources");
    expect(sourceRow().textContent).toContain("Alpha");
    expect(
      container.querySelector("[aria-label='catalog.streaming.signedIn']"),
    ).toBeNull();
  });

  test("shows a signed-in dot instead of the sign-in hint", async () => {
    catalogState.sourceStates.alpha.session = {
      signed_in: true,
      expired: false,
    };
    await renderSidebar(root);
    expect(
      container.querySelector("[aria-label='catalog.streaming.signedIn']"),
    ).not.toBeNull();
    expect(sourceRow().textContent).toContain("Alpha");
  });

  test("treats an expired session as signed out", async () => {
    catalogState.sourceStates.alpha.session = {
      signed_in: true,
      expired: true,
    };
    await renderSidebar(root);
    expect(sourceRow().textContent).toContain("Alpha");
  });

  test("marks the source row as the current page when selected", async () => {
    catalogState.activeView = "streaming";
    catalogState.activeSourceId = "alpha";
    await renderSidebar(root);
    expect(sourceRow().getAttribute("aria-current")).toBe("page");
  });

  test("YouTube row toggles the link popover", async () => {
    await renderSidebar(root);
    rowByText("sidebar.youtubeLink")!.click();
    expect(setYoutubeLinkOpen).toHaveBeenCalledWith(true);
  });

  test("hides disabled sources and the whole section when none are enabled", async () => {
    catalogState.sources[0].enabled = false;
    await renderSidebar(root);
    expect(rowByText("Alpha")).toBeUndefined();
    expect(rowByText("sidebar.youtubeLink")).toBeDefined();
    act(() => root.unmount());
    root = createRoot(container);
    settingsState.youtubeSourceEnabled = false;
    await renderSidebar(root);
    expect(container.textContent).not.toContain("sidebar.onlineSources");
  });
});
