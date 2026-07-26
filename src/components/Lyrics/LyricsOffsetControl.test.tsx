import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { LyricsOffsetControl } from "./LyricsOffsetControl";

const { mockLyricsState } = vi.hoisted(() => ({
  mockLyricsState: {
    songId: "song-1" as string | null,
    offsetMs: 0,
    adjustOffset: vi.fn(),
    resetOffset: vi.fn(),
  },
}));

vi.mock("@/stores/lyrics-store", () => ({
  useLyricsStore: (selector: (state: typeof mockLyricsState) => unknown) =>
    selector(mockLyricsState),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

describe("LyricsOffsetControl", () => {
  beforeEach(() => {
    mockLyricsState.songId = "song-1";
    mockLyricsState.offsetMs = 0;
  });

  test("renders nothing when no song is loaded", () => {
    mockLyricsState.songId = null;

    const markup = renderToStaticMarkup(<LyricsOffsetControl />);

    expect(markup).toBe("");
  });

  test("displays the zero offset at rest", () => {
    const markup = renderToStaticMarkup(<LyricsOffsetControl />);

    expect(markup).toContain("+0.0s");
    expect(markup).toContain("-0.5s");
    expect(markup).toContain("+0.5s");
  });

  test("displays a positive offset with the correct sign", () => {
    mockLyricsState.offsetMs = 1500;

    const markup = renderToStaticMarkup(<LyricsOffsetControl />);

    expect(markup).toContain("+1.5s");
  });

  test("displays a negative offset with the minus sign", () => {
    mockLyricsState.offsetMs = -500;

    const markup = renderToStaticMarkup(<LyricsOffsetControl />);

    expect(markup).toContain("-0.5s");
  });

  test("highlights the offset display with the control-primary color when offset is non-zero", () => {
    mockLyricsState.offsetMs = 1000;

    const markup = renderToStaticMarkup(<LyricsOffsetControl />);

    expect(markup).toContain("text-[var(--color-control-primary)]");
  });

  test("uses dimmer text when offset is zero", () => {
    const markup = renderToStaticMarkup(<LyricsOffsetControl />);

    // The control-primary highlight must be absent at zero offset so the
    // display rests in the dim text color rather than the active highlight
    // color.
    expect(markup).not.toContain("text-[var(--color-control-primary)]");
  });

  test("renders a reset button mirroring the font-size control", () => {
    const markup = renderToStaticMarkup(<LyricsOffsetControl />);

    expect(markup).toContain("lyrics.offsetReset");
    expect(markup).toContain("lyrics.offsetResetShort");
  });
});
