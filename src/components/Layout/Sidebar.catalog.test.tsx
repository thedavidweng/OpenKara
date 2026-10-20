// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { BackendProvider } from "@/lib/backend";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { Sidebar } from "./Sidebar";

const setFilter = vi.fn();
const setActivePlaylist = vi.fn();
const setCatalogView = vi.fn();
const setYoutubeLinkOpen = vi.fn();
const settingsState = {
  hideBatchSeparate: true,
  hideUpgradeAll: true,
  stemMode: "two_stem",
  neteaseSourceEnabled: true,
  youtubeSourceEnabled: true,
};
const catalogState: {
  activeView: "library" | "netease";
  setActiveView: typeof setCatalogView;
  session: { signed_in: boolean; expired: boolean } | null;
  youtubeLinkOpen: boolean;
  setYoutubeLinkOpen: typeof setYoutubeLinkOpen;
} = {
  activeView: "library",
  setActiveView: setCatalogView,
  session: null,
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

function neteaseRow(): HTMLButtonElement {
  return rowByText("sidebar.netease") as HTMLButtonElement;
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
    catalogState.session = null;
    catalogState.youtubeLinkOpen = false;
    settingsState.neteaseSourceEnabled = true;
    settingsState.youtubeSourceEnabled = true;
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  test("All Tracks, Separated, and NetEase switch the catalog view", async () => {
    const backend = createMockBackend();
    await act(async () => {
      root.render(
        <BackendProvider backend={backend}>
          <Sidebar />
        </BackendProvider>,
      );
    });
    (
      container.querySelector(
        "[aria-label='sidebar.allTracks']",
      ) as HTMLButtonElement
    ).click();
    (
      container.querySelector(
        "[aria-label='sidebar.separated']",
      ) as HTMLButtonElement
    ).click();
    neteaseRow().click();
    expect(setCatalogView).toHaveBeenCalledWith("library");
    expect(setCatalogView).toHaveBeenCalledWith("netease");
    expect(setActivePlaylist).toHaveBeenCalledWith(null);
  });

  test("shows the online sources header with sign-in hint when signed out", async () => {
    await renderSidebar(root);
    expect(container.textContent).toContain("sidebar.onlineSources");
    expect(neteaseRow().textContent).toContain("sidebar.neteaseSignIn");
    expect(
      container.querySelector("[aria-label='sidebar.neteaseSignedIn']"),
    ).toBeNull();
  });

  test("shows a signed-in dot instead of the sign-in hint", async () => {
    catalogState.session = { signed_in: true, expired: false };
    await renderSidebar(root);
    expect(
      container.querySelector("[aria-label='sidebar.neteaseSignedIn']"),
    ).not.toBeNull();
    expect(neteaseRow().textContent).not.toContain("sidebar.neteaseSignIn");
  });

  test("treats an expired session as signed out", async () => {
    catalogState.session = { signed_in: true, expired: true };
    await renderSidebar(root);
    expect(neteaseRow().textContent).toContain("sidebar.neteaseSignIn");
  });

  test("marks the NetEase row as the current page when selected", async () => {
    catalogState.activeView = "netease";
    await renderSidebar(root);
    expect(neteaseRow().getAttribute("aria-current")).toBe("page");
  });

  test("YouTube row toggles the link popover", async () => {
    await renderSidebar(root);
    rowByText("sidebar.youtubeLink")!.click();
    expect(setYoutubeLinkOpen).toHaveBeenCalledWith(true);
  });

  test("hides disabled sources and the whole section when none are enabled", async () => {
    settingsState.neteaseSourceEnabled = false;
    await renderSidebar(root);
    expect(rowByText("sidebar.netease")).toBeUndefined();
    expect(rowByText("sidebar.youtubeLink")).toBeDefined();
    act(() => root.unmount());
    root = createRoot(container);
    settingsState.youtubeSourceEnabled = false;
    await renderSidebar(root);
    expect(container.textContent).not.toContain("sidebar.onlineSources");
  });
});
