import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { LyricsEmptyState } from "./LyricsEmptyState";

const { mockPlayerState, mockLyricsState } = vi.hoisted(() => ({
  mockPlayerState: {
    snapshot: null as { song_id: string } | null,
  },
  mockLyricsState: {
    rawLrc: null as string | null,
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: (selector: (state: typeof mockPlayerState) => unknown) =>
    selector(mockPlayerState),
}));

vi.mock("@/stores/lyrics-store", () => ({
  useLyricsStore: (selector: (state: typeof mockLyricsState) => unknown) =>
    selector(mockLyricsState),
}));

vi.mock("./LyricsEditDialog", () => ({
  LyricsEditDialog: () => <div data-testid="lyrics-edit-dialog" />,
}));

describe("LyricsEmptyState", () => {
  test("shows the no-lyrics message", () => {
    mockPlayerState.snapshot = { song_id: "song-1" };
    mockLyricsState.rawLrc = null;

    const markup = renderToStaticMarkup(<LyricsEmptyState />);

    expect(markup).toContain("lyrics.noLyrics");
  });

  test("renders the add-lyrics button when a song is loaded in standard presentation", () => {
    mockPlayerState.snapshot = { song_id: "song-1" };
    mockLyricsState.rawLrc = null;

    const markup = renderToStaticMarkup(<LyricsEmptyState />);

    expect(markup).toContain("lyrics.addLyrics");
    expect(markup).toContain("<button");
  });

  test("hides the add-lyrics button when no song is loaded", () => {
    mockPlayerState.snapshot = null;
    mockLyricsState.rawLrc = null;

    const markup = renderToStaticMarkup(<LyricsEmptyState />);

    expect(markup).toContain("lyrics.noLyrics");
    expect(markup).not.toContain("lyrics.addLyrics");
    expect(markup).not.toContain("<button");
  });

  test("hides the add-lyrics button in audience presentation", () => {
    mockPlayerState.snapshot = { song_id: "song-1" };
    mockLyricsState.rawLrc = "[00:00.00]some lyric";

    const markup = renderToStaticMarkup(
      <LyricsEmptyState presentation="audience" />,
    );

    expect(markup).toContain("lyrics.noLyrics");
    expect(markup).not.toContain("lyrics.addLyrics");
    expect(markup).not.toContain("<button");
  });

  test("applies pointer-events-none when coexisting with a drag region", () => {
    mockPlayerState.snapshot = { song_id: "song-1" };
    mockLyricsState.rawLrc = null;

    const markup = renderToStaticMarkup(
      <LyricsEmptyState pointerEventsCoexistWithDragRegion />,
    );

    expect(markup).toContain("pointer-events-none");
    expect(markup).toContain("pointer-events-auto");
  });

  test("does not apply pointer-events-none in standard mode", () => {
    mockPlayerState.snapshot = { song_id: "song-1" };
    mockLyricsState.rawLrc = null;

    const markup = renderToStaticMarkup(<LyricsEmptyState />);

    expect(markup).not.toContain("pointer-events-none");
  });
});
