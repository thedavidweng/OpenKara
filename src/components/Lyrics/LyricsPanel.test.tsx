// @vitest-environment jsdom

import { renderToStaticMarkup } from "react-dom/server";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { AirPlayOutputStateEvent } from "@/types/ipc";
import type { LyricLine } from "@/types/ipc";
import { LyricsPanel } from "./LyricsPanel";

function line(input: Omit<LyricLine, "bg_words" | "section">): LyricLine {
  return {
    ...input,
    bg_words: null,
    section: null,
  };
}

const {
  mockPlayerState,
  mockLyricsState,
  mockSettingsState,
  mockSelectCurrentPositionMs,
} = vi.hoisted(() => ({
  mockPlayerState: {
    snapshot: {
      song_id: "song-1",
      is_playing: true,
      state: "playing",
    },
    positionMs: 4000,
    playingSinceMs: 1000,
    airPlayOutput: {
      active: false,
      audioActive: false,
      routeName: null,
      mode: "idle",
      phase: "idle",
      detail: null,
      displayedPositionMs: null,
      streamGeneration: 0,
      latencyMs: null,
    } as AirPlayOutputStateEvent,
    localAudienceOutputActive: false,
    airPlayPlainTextPagePending: false,
    airPlayPlainTextPagePendingDirection: null as "prev" | "next" | null,
  },
  mockLyricsState: {
    lines: [
      line({
        time_ms: 0,
        text: "line one",
        words: null,
      }),
    ],
    activeLineIndex: 0,
    offsetMs: 0,
    isLoading: false,
    rawLrc: "[00:00.00]line one",
    romanizedLines: [],
    isRomanizing: false,
    showRomanized: false,
    toggleRomanized: vi.fn(),
    songId: "song-1",
    adjustOffset: vi.fn(),
  } as {
    lines: LyricLine[];
    activeLineIndex: number;
    offsetMs: number;
    isLoading: boolean;
    rawLrc: string;
    romanizedLines: string[];
    isRomanizing: boolean;
    showRomanized: boolean;
    toggleRomanized: ReturnType<typeof vi.fn>;
    songId: string;
    adjustOffset: ReturnType<typeof vi.fn>;
  },
  mockSettingsState: {
    lyricsFontStep: 0,
    adjustLyricsFontStep: vi.fn(),
    resetLyricsFontStep: vi.fn(),
  },
  mockSelectCurrentPositionMs: vi.fn(
    (state: { positionMs: number }) => state.positionMs,
  ),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { changeLanguage: vi.fn() },
  }),
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: Object.assign(
    (selector: (state: typeof mockPlayerState) => unknown) =>
      selector(mockPlayerState),
    {
      getState: () => mockPlayerState,
    },
  ),
  selectCurrentPositionMs: mockSelectCurrentPositionMs,
}));

vi.mock("@/stores/lyrics-store", () => ({
  useLyricsStore: Object.assign(
    (selector: (state: typeof mockLyricsState) => unknown) =>
      selector(mockLyricsState),
    {
      getState: () => mockLyricsState,
    },
  ),
}));

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: (selector: (state: typeof mockSettingsState) => unknown) =>
    selector(mockSettingsState),
}));

describe("LyricsPanel contextual reveal", () => {
  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    vi.stubGlobal(
      "requestAnimationFrame",
      vi.fn((callback: FrameRequestCallback) => {
        void callback;
        return 1;
      }),
    );
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    Object.defineProperty(HTMLElement.prototype, "scrollTo", {
      configurable: true,
      value: vi.fn(),
    });

    mockPlayerState.snapshot = {
      song_id: "song-1",
      is_playing: true,
      state: "playing",
    };
    mockPlayerState.positionMs = 4000;
    mockPlayerState.airPlayOutput = {
      active: false,
      audioActive: false,
      routeName: null,
      mode: "idle",
      phase: "idle",
      detail: null,
      displayedPositionMs: null,
      streamGeneration: 0,
      latencyMs: null,
    };
    mockPlayerState.localAudienceOutputActive = false;
    mockPlayerState.airPlayPlainTextPagePending = false;
    mockPlayerState.airPlayPlainTextPagePendingDirection = null;
    mockSelectCurrentPositionMs.mockImplementation((state) => state.positionMs);

    mockLyricsState.lines = [
      line({
        time_ms: 0,
        text: "line one",
        words: null,
      }),
    ];
    mockLyricsState.activeLineIndex = 0;
    mockLyricsState.offsetMs = 0;
    mockLyricsState.isLoading = false;
    mockLyricsState.rawLrc = "[00:00.00]line one";
    mockLyricsState.romanizedLines = [];
    mockLyricsState.isRomanizing = false;
    mockLyricsState.showRomanized = false;
    mockLyricsState.toggleRomanized.mockReset();
    mockLyricsState.songId = "song-1";
    mockLyricsState.adjustOffset.mockReset();
    mockSettingsState.lyricsFontStep = 0;
  });

  afterEach(() => {
    document.body.innerHTML = "";
    vi.unstubAllGlobals();
  });

  test("renders utility chrome in an overlay layer without layout controls at rest", () => {
    const markup = renderToStaticMarkup(<LyricsPanel />);

    expect(markup).toContain("contextual-reveal absolute right-4 top-4");
    expect(markup).toContain("absolute inset-x-0 bottom-0");
    expect(markup).not.toContain('data-visible="true"');
  });

  test("uses the spacious stage lyric layout for standard presentation", () => {
    const markup = renderToStaticMarkup(<LyricsPanel />);

    expect(markup).toContain('data-lyrics-visual-variant="stage-layout"');
    expect(markup).toContain('data-native-lyrics-layout="true"');
  });

  test("keeps lyric utility chrome visible when offset is non-zero", () => {
    mockLyricsState.offsetMs = 500;

    const markup = renderToStaticMarkup(<LyricsPanel />);

    expect(markup).toContain('data-visible="true"');
    expect(markup).toContain("+0.5s");
  });

  test("keeps lyric utility chrome visible when font size is non-default", () => {
    mockSettingsState.lyricsFontStep = 1;

    const markup = renderToStaticMarkup(<LyricsPanel />);

    expect(markup).toContain('data-visible="true"');
    expect(markup).toContain("lyrics.fontSizeResetShort");
  });

  test("renders plain-text lyrics at full brightness when no timestamps exist", () => {
    mockLyricsState.lines = [
      line({
        time_ms: 0,
        text: "line one",
        words: null,
      }),
      line({
        time_ms: 0,
        text: "line two",
        words: null,
      }),
    ];

    const markup = renderToStaticMarkup(<LyricsPanel />);

    expect(markup).toContain('text-white">line one</span>');
    expect(markup).toContain('text-white">line two</span>');
    expect(markup).not.toContain('text-[var(--color-active)]">line one</span>');
  });

  test("shows remote paging controls for plain-text lyrics when a remote audience target exists", () => {
    mockPlayerState.localAudienceOutputActive = true;

    const markup = renderToStaticMarkup(<LyricsPanel />);

    expect(markup).toContain("plain-text-page-prev");
    expect(markup).toContain("plain-text-page-next");
    expect(markup).toContain("lyrics.previousPage");
    expect(markup).toContain("lyrics.nextPage");
  });

  test("shows pending feedback on the active AirPlay paging button while waiting for remote display", () => {
    mockPlayerState.airPlayOutput = {
      active: true,
      audioActive: true,
      routeName: "Living Room TV",
      mode: "lyrics",
      phase: "playing",
      detail: null,
      displayedPositionMs: 1250,
      streamGeneration: 3,
      latencyMs: 900,
    } satisfies AirPlayOutputStateEvent;
    mockPlayerState.airPlayPlainTextPagePending = true;
    mockPlayerState.airPlayPlainTextPagePendingDirection = "next";

    const markup = renderToStaticMarkup(<LyricsPanel />);

    expect(markup).toContain('data-airplay-page-pending="true"');
    expect(markup).toContain("animate-spin");
    expect(markup).toContain('disabled=""');
  });

  test("keeps remote paging controls hidden when no remote audience target exists", () => {
    const markup = renderToStaticMarkup(<LyricsPanel />);

    expect(markup).not.toContain("plain-text-page-prev");
    expect(markup).not.toContain("plain-text-page-next");
  });

  test("keeps remote paging controls hidden when AirPlay is not actually active", () => {
    mockPlayerState.airPlayOutput = {
      active: false,
      audioActive: true,
      routeName: "Living Room TV",
      mode: "lyrics",
      phase: "buffering",
      detail: "waiting_for_route",
      displayedPositionMs: null,
      streamGeneration: 2,
      latencyMs: null,
    } satisfies AirPlayOutputStateEvent;

    const markup = renderToStaticMarkup(<LyricsPanel />);

    expect(markup).not.toContain("plain-text-page-prev");
    expect(markup).not.toContain("plain-text-page-next");
  });

  test("uses fullscreen audience layout without edit chrome when requested", () => {
    const markup = renderToStaticMarkup(
      <LyricsPanel presentation="audience" />,
    );

    expect(markup).toContain("max-width:min(92vw, 1600px)");
    expect(markup).toContain("min-h-full");
    expect(markup).not.toContain("contextual-reveal absolute right-4 top-4");
    expect(markup).not.toContain("absolute inset-x-0 bottom-0");
  });

  test("uses a passive one-line empty state in audience presentation", () => {
    mockLyricsState.lines = [];

    const markup = renderToStaticMarkup(
      <LyricsPanel presentation="audience" />,
    );

    expect(markup).toContain("lyrics.noLyrics");
    expect(markup).not.toContain("lyrics.addLyrics");
    expect(markup).not.toContain("<button");
  });

  test("renders stable line markers for timed lyrics auto-scroll targeting", () => {
    mockLyricsState.lines = [
      line({
        time_ms: 1000,
        text: "line one",
        words: null,
      }),
      line({
        time_ms: 2000,
        text: "line two",
        words: null,
      }),
    ];
    mockLyricsState.activeLineIndex = 1;

    const markup = renderToStaticMarkup(<LyricsPanel />);

    expect(markup).toContain('data-lyrics-line-index="0"');
    expect(markup).toContain('data-lyrics-line-index="1"');
  });

  test("initializes line springs for timed lyrics", () => {
    mockLyricsState.lines = [
      line({
        time_ms: 1000,
        text: "line one",
        words: null,
      }),
      line({
        time_ms: 2000,
        text: "line two",
        words: null,
      }),
    ];
    mockLyricsState.activeLineIndex = 0;

    const markup = renderToStaticMarkup(<LyricsPanel />);

    expect(markup).toContain('data-lyrics-line-index="0"');
    expect(markup).toContain('data-lyrics-line-index="1"');
    expect(markup).toContain("transform:scale(");
  });

  test("restarts the spring RAF loop when the song changes without active line movement", async () => {
    mockLyricsState.lines = [
      line({
        time_ms: 1000,
        text: "first song line one",
        words: null,
      }),
      line({
        time_ms: 2000,
        text: "first song line two",
        words: null,
      }),
    ];
    mockLyricsState.activeLineIndex = 0;
    const requestAnimationFrameMock = vi.mocked(requestAnimationFrame);
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(<LyricsPanel />);
    });

    const callsAfterMount = requestAnimationFrameMock.mock.calls.length;
    expect(callsAfterMount).toBeGreaterThan(0);

    mockPlayerState.snapshot = {
      song_id: "song-2",
      is_playing: true,
      state: "playing",
    };
    mockLyricsState.rawLrc = "[00:00.00]second song line one";
    mockLyricsState.lines = [
      line({
        time_ms: 1000,
        text: "second song line one",
        words: null,
      }),
      line({
        time_ms: 2000,
        text: "second song line two",
        words: null,
      }),
    ];

    await act(async () => {
      root.render(<LyricsPanel />);
    });

    expect(requestAnimationFrameMock.mock.calls.length).toBeGreaterThan(
      callsAfterMount,
    );

    await act(async () => {
      root.unmount();
    });
  });

  test("uses the extrapolated playback clock for standard word highlighting", () => {
    mockPlayerState.positionMs = 1000;
    mockSelectCurrentPositionMs.mockReturnValue(1600);
    mockLyricsState.lines = [
      line({
        time_ms: 1000,
        text: "alpha beta gamma",
        words: [
          { text: "alpha", time_ms: 1000, end_ms: 1500 },
          { text: "beta", time_ms: 1500, end_ms: 2000 },
          { text: "gamma", time_ms: 2000, end_ms: 2500 },
        ],
      }),
    ];
    mockLyricsState.activeLineIndex = 0;

    const markup = renderToStaticMarkup(<LyricsPanel />);

    expect(markup).toContain("text-white/45");
    expect(markup).toContain("text-white");
    expect(markup).toContain("text-white/50");
  });
});
