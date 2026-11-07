// @vitest-environment jsdom
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  expect,
  test,
  vi,
} from "vite-plus/test";
import { create } from "zustand";
import { createMockBackend } from "@/lib/backend/mock-backend";
import {
  createCatalogStore,
  type CatalogState,
  type CatalogStore,
} from "@/stores/catalog-store";
import type { Song, StreamingTrack } from "@/types/ipc";
import { source } from "@/test-utils/streaming-source";
import { SearchBox } from "@/components/Library/SearchBox";
import { GlobalSearchView } from "./GlobalSearchView";

const shared = vi.hoisted(() => ({ store: null as unknown }));
vi.mock("@/stores/catalog-store", async (original) => {
  const actual = await original<{
    useCatalogStore: CatalogStore;
    createCatalogStore: typeof createCatalogStore;
  }>();
  return {
    ...actual,
    useCatalogStore: Object.assign(
      (selector: (state: CatalogState) => unknown) =>
        (shared.store as CatalogStore)(selector),
      { getState: () => (shared.store as CatalogStore).getState() },
    ),
  };
});
const library = vi.hoisted(() => ({
  state: null as unknown,
}));
vi.mock("@/stores/library-store", () => ({
  useLibraryStore: (selector: (state: unknown) => unknown) =>
    (library.state as (selector: (state: unknown) => unknown) => unknown)(
      selector,
    ),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
  initReactI18next: { type: "3rdParty", init() {} },
}));

interface LibraryState {
  searchQuery: string;
  songsQuery: string;
  songs: Song[];
  setSearchQuery: (query: string) => void;
}
const libraryStore = create<LibraryState>((set) => ({
  searchQuery: "",
  songsQuery: "",
  songs: [],
  setSearchQuery: (searchQuery) => set({ searchQuery }),
}));
library.state = libraryStore;

const track = (sourceId: string): StreamingTrack => ({
  source_id: sourceId,
  remote_track_id: "same-id",
  title: `${sourceId} song`,
  artist: "Artist",
  album: null,
  duration_ms: 200_000,
  refusal: null,
  library_song_id: null,
  cover_url: null,
});
const song = (index: number): Song => ({
  hash: `hash-${index}`,
  file_path: null,
  audio_source_kind: "original",
  cdg_path: null,
  media_g_container: null,
  instrumental: false,
  language: null,
  title: `Library song ${index}`,
  artist: "Artist",
  album: null,
  duration_ms: 180_000,
  cover_art: null,
  has_cover_art: false,
  artwork_thumb_path: null,
  imported_at: 0,
  original_ext: null,
});
const signedOut = async (source_id: string) => ({
  source_id,
  signed_in: false,
  display_name: null,
  expired: false,
});

let store: CatalogStore;
const viewportProps = ["offsetHeight", "clientHeight"] as const;
const originalViewport = viewportProps.map((prop) =>
  Object.getOwnPropertyDescriptor(HTMLElement.prototype, prop),
);
beforeAll(() => {
  for (const prop of viewportProps) {
    Object.defineProperty(HTMLElement.prototype, prop, {
      configurable: true,
      get: () => 600,
    });
  }
});
afterAll(() => {
  viewportProps.forEach((prop, index) => {
    const original = originalViewport[index];
    if (original) Object.defineProperty(HTMLElement.prototype, prop, original);
  });
});
beforeEach(() => {
  libraryStore.setState({ searchQuery: "", songsQuery: "", songs: [] });
});
afterEach(cleanup);

test("one search covers every ready source, and an import uses the track's own source", async () => {
  const search = vi.fn(async (id: string) => ({
    tracks: [track(id)],
    next_cursor: null,
  }));
  const start = vi.fn().mockReturnValue(new Promise(() => {}));
  store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          getStreamingSession: signedOut,
          searchStreamingSource: search,
          startStreamingImport: start,
        },
      },
    }),
  );
  shared.store = store;
  store
    .getState()
    .applySources([
      source("alpha", ["search", "import"]),
      source("beta", ["search", "import"]),
      source("gamma", ["liked_tracks"]),
      source("delta", ["search"], { enabled: false }),
    ]);

  await act(() => store.getState().searchEverywhere("  hello "));
  render(<GlobalSearchView />);

  expect(store.getState().activeView).toBe("search");
  expect(search.mock.calls.map((call) => call.slice(0, 3))).toEqual([
    ["alpha", "hello", null],
    ["beta", "hello", null],
  ]);
  expect(screen.queryByRole("region", { name: "gamma" })).toBeNull();
  expect(screen.queryByRole("region", { name: "delta" })).toBeNull();
  const beta = screen.getByRole("region", { name: "beta" });
  fireEvent.click(
    within(beta).getByRole("button", {
      name: "catalog.streaming.importTrackLabel",
    }),
  );
  expect(start.mock.calls[0].slice(0, 3)).toEqual(["beta", ["same-id"], null]);
  expect(store.getState().importRun?.label).toBe("beta song");
});

test("a signed-out or failing source does not hide the other results, and retry recovers", async () => {
  const search = vi
    .fn()
    .mockRejectedValueOnce(new Error("offline"))
    .mockImplementation(async (id: string) => ({
      tracks: [track(id)],
      next_cursor: null,
    }));
  store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          getStreamingSession: signedOut,
          searchStreamingSource: search,
        },
      },
    }),
  );
  shared.store = store;
  store
    .getState()
    .applySources([
      source("alpha", ["search"], { requiresSearchAuth: true }),
      source("beta", ["search"]),
    ]);
  libraryStore.setState({
    searchQuery: "hello",
    songsQuery: "hello",
    songs: Array.from({ length: 20 }, (_, i) => song(i)),
  });

  await act(() => store.getState().searchEverywhere("hello"));
  render(<GlobalSearchView />);

  expect(search).toHaveBeenCalledTimes(1);
  expect(
    within(screen.getByRole("region", { name: "alpha" })).getByText(
      "globalSearch.signInToSearch",
    ),
  ).toBeTruthy();
  const library = screen.getByRole("region", { name: "globalSearch.library" });
  const shown = within(library).getAllByRole("button");
  expect(shown.length).toBeGreaterThan(0);
  expect(shown.length).toBeLessThan(20);
  expect(within(library).getByText("globalSearch.libraryMore")).toBeTruthy();

  const beta = screen.getByRole("region", { name: "beta" });
  expect(within(beta).getByRole("alert")).toBeTruthy();
  fireEvent.click(within(beta).getByRole("button", { name: "common.retry" }));
  await waitFor(() => expect(within(beta).getByText("beta song")).toBeTruthy());
  expect(search.mock.calls[1].slice(0, 3)).toEqual(["beta", "hello", null]);
});

test("the library part waits for the library search instead of showing the whole library", async () => {
  store = createCatalogStore(createMockBackend());
  shared.store = store;
  libraryStore.setState({
    searchQuery: "hello",
    songsQuery: "",
    songs: [song(1)],
  });
  await act(() => store.getState().searchEverywhere("hello"));
  render(<GlobalSearchView />);

  const library = screen.getByRole("region", { name: "globalSearch.library" });
  expect(within(library).getByRole("status").textContent).toBe(
    "globalSearch.searching",
  );
  expect(within(library).queryByRole("button")).toBeNull();
  act(() => libraryStore.setState({ songsQuery: "hello" }));
  expect(within(library).getAllByRole("button")).toHaveLength(1);
});

test("Enter in the sidebar search searches sources, but not while an input method composes", async () => {
  store = createCatalogStore(
    createMockBackend({
      overrides: {
        catalog: {
          getStreamingSession: signedOut,
          searchStreamingSource: async () => ({
            tracks: [],
            next_cursor: null,
          }),
        },
      },
    }),
  );
  shared.store = store;
  const searchEverywhere = vi.spyOn(store.getState(), "searchEverywhere");
  store.setState({ searchEverywhere });
  render(<SearchBox />);
  const input = screen.getByRole("textbox", { name: "common.search" });

  fireEvent.change(input, { target: { value: "hello" } });
  expect(
    screen.queryByRole("button", { name: "globalSearch.searchSources" }),
  ).toBeNull();

  act(() => store.getState().applySources([source("alpha", ["search"])]));
  fireEvent.keyDown(input, { key: "Enter", isComposing: true });
  expect(searchEverywhere).not.toHaveBeenCalled();
  fireEvent.keyDown(input, { key: "Enter" });
  expect(searchEverywhere).toHaveBeenCalledWith("hello");
  fireEvent.click(
    screen.getByRole("button", { name: "globalSearch.searchSources" }),
  );
  expect(searchEverywhere).toHaveBeenCalledTimes(2);
});
