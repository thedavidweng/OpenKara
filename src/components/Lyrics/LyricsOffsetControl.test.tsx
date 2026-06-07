import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { LyricsOffsetControl } from "./LyricsOffsetControl";

const { mockLyricsState } = vi.hoisted(() => ({
  mockLyricsState: {
    songId: "song-1" as string | null,
    offsetMs: 0,
    adjustOffset: vi.fn(),
  },
}));

vi.mock("@/stores/lyrics-store", () => ({
  useLyricsStore: (selector: (state: typeof mockLyricsState) => unknown) =>
    selector(mockLyricsState),
}));

describe("LyricsOffsetControl", () => {
  test("renders nothing when no song is loaded", () => {
    mockLyricsState.songId = null;
    mockLyricsState.offsetMs = 0;

    const markup = renderToStaticMarkup(<LyricsOffsetControl />);

    expect(markup).toBe("");
  });

  test("displays the zero offset at rest", () => {
    mockLyricsState.songId = "song-1";
    mockLyricsState.offsetMs = 0;

    const markup = renderToStaticMarkup(<LyricsOffsetControl />);

    expect(markup).toContain("+0.0s");
    expect(markup).toContain("-0.5s");
    expect(markup).toContain("+0.5s");
  });

  test("displays a positive offset with the correct sign", () => {
    mockLyricsState.songId = "song-1";
    mockLyricsState.offsetMs = 1500;

    const markup = renderToStaticMarkup(<LyricsOffsetControl />);

    expect(markup).toContain("+1.5s");
  });

  test("displays a negative offset with the minus sign", () => {
    mockLyricsState.songId = "song-1";
    mockLyricsState.offsetMs = -500;

    const markup = renderToStaticMarkup(<LyricsOffsetControl />);

    expect(markup).toContain("-0.5s");
  });

  test("highlights the offset display when offset is non-zero", () => {
    mockLyricsState.songId = "song-1";
    mockLyricsState.offsetMs = 1000;

    const markup = renderToStaticMarkup(<LyricsOffsetControl />);

    expect(markup).toContain("text-white");

    mockLyricsState.offsetMs = 0;
  });

  test("uses dimmer text when offset is zero", () => {
    mockLyricsState.songId = "song-1";
    mockLyricsState.offsetMs = 0;

    const markup = renderToStaticMarkup(<LyricsOffsetControl />);

    expect(markup).toContain("text-[var(--color-text)]");
  });
});
