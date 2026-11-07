// @vitest-environment jsdom
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import {
  afterEach,
  beforeEach,
  describe,
  expect,
  test,
  vi,
} from "vite-plus/test";
import { useCatalogStore } from "@/stores/catalog-store";
import { source } from "@/test-utils/streaming-source";
import type { StreamingTrack } from "@/types/ipc";
import { PlayableMatchDialog } from "./PlayableMatchDialog";

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
  album: "Album",
  duration_ms: 61000,
  refusal: null,
  library_song_id: null,
  cover_url: null,
  isrc: null,
  ...overrides,
});

const original = track("meta", "orig", { isrc: "TWA010000001" });
const first = track("files", "one", { isrc: "TWA010000001" });
const second = track("web", "two");

const confirmPlayableMatch = vi.fn();
const closePlayableMatch = vi.fn();

beforeEach(() => {
  confirmPlayableMatch.mockReset();
  closePlayableMatch.mockReset();
  useCatalogStore.setState({
    sources: [source("meta", []), source("files", []), source("web", [])],
    confirmPlayableMatch,
    closePlayableMatch,
    playableMatch: null,
  });
});
afterEach(() => {
  cleanup();
  useCatalogStore.setState({ playableMatch: null });
});

const open = (status: "loading" | "ready", candidates: StreamingTrack[] = []) =>
  act(() =>
    useCatalogStore.setState({
      playableMatch: { original, status, candidates },
    }),
  );

describe("PlayableMatchDialog", () => {
  test("shows a status while the search runs", () => {
    render(<PlayableMatchDialog />);
    open("loading");

    expect(
      screen.getByRole("dialog", {
        name: "catalog.streaming.playableMatch.title",
      }),
    ).toBeTruthy();
    expect(screen.getByRole("status").textContent).toBe(
      "catalog.streaming.playableMatch.loading",
    );
    expect(
      (
        screen.getByText(
          "catalog.streaming.playableMatch.import",
        ) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
  });

  test("explains an empty result", () => {
    render(<PlayableMatchDialog />);
    open("ready");

    expect(screen.getByRole("status").textContent).toBe(
      "catalog.streaming.playableMatch.empty",
    );
  });

  test("preselects the first candidate, marks the same ISRC, and imports the choice", () => {
    render(<PlayableMatchDialog />);
    open("ready", [first, second]);

    const radios = screen.getAllByRole("radio") as HTMLInputElement[];
    expect(radios.map((radio) => radio.checked)).toEqual([true, false]);
    expect(
      screen.getAllByText("catalog.streaming.playableMatch.sameIsrc"),
    ).toHaveLength(1);

    fireEvent.click(radios[1]);
    expect(radios.map((radio) => radio.checked)).toEqual([false, true]);
    fireEvent.click(screen.getByText("catalog.streaming.playableMatch.import"));
    expect(confirmPlayableMatch).toHaveBeenCalledWith(second);
  });

  test("Escape closes the dialog and focus returns to the invoking button", async () => {
    render(
      <>
        <button type="button">Find</button>
        <PlayableMatchDialog />
      </>,
    );
    const invoker = screen.getByText("Find");
    invoker.focus();
    open("ready", [first]);
    await vi.waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("radio")),
    );

    fireEvent.keyDown(document, { key: "Escape" });
    expect(closePlayableMatch).toHaveBeenCalledTimes(1);

    act(() => useCatalogStore.setState({ playableMatch: null }));
    await vi.waitFor(() => expect(document.activeElement).toBe(invoker));
  });
});
