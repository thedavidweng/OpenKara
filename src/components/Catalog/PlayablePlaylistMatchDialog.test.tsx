// @vitest-environment jsdom
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import {
  afterEach,
  beforeEach,
  describe,
  expect,
  test,
  vi,
} from "vite-plus/test";
import { onlinePreviewQueueId } from "@/playback";
import {
  useCatalogStore,
  type PlaylistMatchRow,
  type PlaylistMatchState,
} from "@/stores/catalog-store";
import { source } from "@/test-utils/streaming-source";
import type { StreamingTrack } from "@/types/ipc";
import { PlayablePlaylistMatchDialog } from "./PlayablePlaylistMatchDialog";

vi.mock("react-i18next", () => ({
  initReactI18next: { type: "3rdParty", init: () => {} },
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options ? `${key}|${JSON.stringify(options)}` : key,
  }),
}));

const track = (
  sourceId: string,
  id: string,
  overrides: Partial<StreamingTrack> = {},
): StreamingTrack => ({
  source_id: sourceId,
  remote_track_id: id,
  title: `Song ${id}`,
  artist: "Artist",
  album: null,
  duration_ms: 61000,
  refusal: null,
  library_song_id: null,
  cover_url: null,
  isrc: null,
  ...overrides,
});
const key = (candidate: StreamingTrack) =>
  onlinePreviewQueueId(candidate.source_id, candidate.remote_track_id);

const one = track("files", "one");
const remembered = track("web", "two");
const rows: PlaylistMatchRow[] = [
  {
    original: track("meta", "a"),
    options: [one, track("web", "other")],
    rememberedKey: null,
    choice: key(one),
  },
  {
    original: track("meta", "b"),
    options: [remembered],
    rememberedKey: key(remembered),
    choice: key(remembered),
  },
  {
    original: track("meta", "c"),
    options: [],
    rememberedKey: null,
    choice: null,
  },
];
const match = (overrides: Partial<PlaylistMatchState> = {}) =>
  ({
    sourceId: "meta",
    remoteId: "pl",
    name: "Road trip",
    phase: "review",
    searched: 3,
    searchTotal: 3,
    rows,
    addToPlaylist: true,
    finishedBefore: 0,
    importTotal: 0,
    cancelling: false,
    imported: 0,
    playlistName: null,
    failures: [],
    ...overrides,
  }) satisfies PlaylistMatchState;

const confirmPlaylistMatches = vi.fn();
const closePlaylistMatch = vi.fn();

beforeEach(() => {
  confirmPlaylistMatches.mockReset();
  closePlaylistMatch.mockReset();
  useCatalogStore.setState({
    sources: [source("meta", []), source("files", []), source("web", [])],
    confirmPlaylistMatches,
    closePlaylistMatch,
    playlistMatch: null,
  });
});
afterEach(() => {
  cleanup();
  useCatalogStore.setState({ playlistMatch: null });
});

const open = (state: PlaylistMatchState) =>
  act(() => useCatalogStore.setState({ playlistMatch: state }));

describe("PlayablePlaylistMatchDialog", () => {
  test("shows search progress as a status", () => {
    render(<PlayablePlaylistMatchDialog />);
    open(match({ phase: "searching", searched: 3, searchTotal: 12, rows: [] }));

    expect(screen.getByRole("status").textContent).toBe(
      'catalog.streaming.playlistMatch.searching|{"current":4,"total":12}',
    );
  });

  test("names each select after its song, preselects, and counts the songs to import", () => {
    render(<PlayablePlaylistMatchDialog />);
    open(match());

    const selects = screen.getAllByRole("combobox") as HTMLSelectElement[];
    expect(selects).toHaveLength(2);
    expect(selects[0].getAttribute("aria-label")).toContain("Song a");
    expect(selects[0].value).toBe(key(one));
    expect(selects[1].value).toBe(key(remembered));
    expect(within(selects[1]).getAllByRole("option")[0].textContent).toContain(
      "catalog.streaming.playlistMatch.remembered",
    );
    expect(
      screen.getByText("catalog.streaming.playlistMatch.noCandidate"),
    ).toBeTruthy();
    expect(screen.getByRole("status").textContent).toBe(
      'catalog.streaming.playlistMatch.summary|{"selected":2,"total":3}',
    );
    expect(
      (
        screen.getByRole("checkbox", {
          name: "catalog.streaming.playlistMatch.addToPlaylist",
        }) as HTMLInputElement
      ).checked,
    ).toBe(true);
  });

  test("skipping a song lowers the count and an empty selection disables Import", () => {
    render(<PlayablePlaylistMatchDialog />);
    open(match());
    const [first, second] = screen.getAllByRole(
      "combobox",
    ) as HTMLSelectElement[];

    fireEvent.change(first, { target: { value: "" } });
    expect(screen.getByRole("status").textContent).toContain('"selected":1');
    fireEvent.change(second, { target: { value: "" } });
    expect(screen.getByRole("status").textContent).toContain('"selected":0');
    const importButton = screen.getByText(
      "catalog.streaming.playlistMatch.import",
    ) as HTMLButtonElement;
    expect(importButton.disabled).toBe(true);
  });

  test("Import confirms the review", () => {
    render(<PlayablePlaylistMatchDialog />);
    open(match());

    fireEvent.click(screen.getByText("catalog.streaming.playlistMatch.import"));

    expect(confirmPlaylistMatches).toHaveBeenCalledTimes(1);
  });

  test("lists the songs that were not imported when it is done", () => {
    render(<PlayablePlaylistMatchDialog />);
    open(
      match({
        phase: "done",
        imported: 2,
        failures: [
          {
            title: "Song c",
            artist: "Artist",
            reason: "noCandidate",
            refusal: null,
          },
        ],
      }),
    );

    const failed = screen.getByRole("region");
    expect(failed.textContent).toContain("Song c");
    expect(failed.textContent).toContain(
      "catalog.streaming.playlistMatch.noCandidate",
    );
  });

  test("Escape closes the dialog and focus returns to the invoking button", async () => {
    render(
      <>
        <button type="button">Find</button>
        <PlayablePlaylistMatchDialog />
      </>,
    );
    const invoker = screen.getByText("Find");
    invoker.focus();
    open(match());
    await vi.waitFor(() =>
      expect(document.activeElement).toBe(screen.getAllByRole("combobox")[0]),
    );

    fireEvent.keyDown(document, { key: "Escape" });
    expect(closePlaylistMatch).toHaveBeenCalledTimes(1);

    act(() => useCatalogStore.setState({ playlistMatch: null }));
    await vi.waitFor(() => expect(document.activeElement).toBe(invoker));
  });
});
