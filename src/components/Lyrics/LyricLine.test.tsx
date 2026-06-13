// @vitest-environment jsdom

import { renderToStaticMarkup } from "react-dom/server";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test, vi, beforeEach, afterEach } from "vitest";
import { LyricLine } from "./LyricLine";

const { mockPlayerState, mockSeek } = vi.hoisted(() => ({
  mockPlayerState: {
    snapshot: {
      is_playing: true,
    },
  },
  mockSeek: vi.fn(),
}));

const { mockControllerInstances } = vi.hoisted(() => ({
  mockControllerInstances: [] as Array<{
    activateLine: ReturnType<typeof vi.fn>;
    setTargetAlpha: ReturnType<typeof vi.fn>;
    setCurrentAlpha: ReturnType<typeof vi.fn>;
    update: ReturnType<typeof vi.fn>;
    deactivateLine: ReturnType<typeof vi.fn>;
    destroy: ReturnType<typeof vi.fn>;
  }>,
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: (
    selector: (state: {
      seek: typeof mockSeek;
      snapshot: typeof mockPlayerState.snapshot;
    }) => unknown,
  ) => selector({ seek: mockSeek, snapshot: mockPlayerState.snapshot }),
}));

vi.mock("./karaoke-fill", () => ({
  KaraokeFillController: vi.fn().mockImplementation(function () {
    const controller = {
      activateLine: vi.fn(),
      setTargetAlpha: vi.fn(),
      setCurrentAlpha: vi.fn(),
      update: vi.fn(),
      deactivateLine: vi.fn(),
      destroy: vi.fn(),
    };
    mockControllerInstances.push(controller);
    return controller;
  }),
}));

describe("LyricLine", () => {
  beforeEach(() => {
    mockControllerInstances.length = 0;
    mockPlayerState.snapshot.is_playing = true;
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("does not render plain-text lyrics as clickable", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        line={{
          time_ms: 0,
          text: "plain line",
          words: null,
          bg_words: null,
          section: null,
        }}
        state="plain"
        adjustedMs={0}
        lyricsFontStep={0}
      />,
    );

    expect(markup).not.toContain("cursor-pointer");
    expect(markup).not.toContain("group-hover/line:underline");
  });

  test("renders seekable lines with cursor-pointer and hover highlight", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        line={{
          time_ms: 1000,
          text: "seekable line",
          words: null,
          bg_words: null,
          section: null,
        }}
        state="future"
        adjustedMs={0}
        lyricsFontStep={0}
      />,
    );

    expect(markup).toContain("cursor-pointer");
    expect(markup).toContain("group-hover/line:-translate-y-px");
    expect(markup).toContain("group-hover/line:[text-shadow:");
    expect(markup).not.toContain("group-hover/line:bg-white/10");
  });

  test("renders word-level states for the active line without changing lyric timing behavior", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        line={{
          time_ms: 1000,
          text: "alpha beta gamma",
          words: [
            { text: "alpha", time_ms: 1000, end_ms: 1500 },
            { text: "beta", time_ms: 1500, end_ms: 2000 },
            { text: "gamma", time_ms: 2000, end_ms: 2500 },
          ],
          bg_words: null,
          section: null,
        }}
        state="active"
        adjustedMs={1600}
        lyricsFontStep={0}
      />,
    );

    expect(markup).toContain("text-white/45");
    expect(markup).toContain("text-white");
    expect(markup).toContain("text-white/50");
  });

  test("uses the configured font scale without changing the lyric state logic", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        line={{
          time_ms: 1000,
          text: "scaled line",
          words: null,
          bg_words: null,
          section: null,
        }}
        state="active"
        adjustedMs={1000}
        presentation="audience"
        lyricsFontStep={2}
      />,
    );

    expect(markup).toContain("font-size:96px");
    expect(markup).toContain("color:rgba(255, 255, 255, 1)");
  });

  test("renders bg_words with slide-in styles when line is active", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        line={{
          time_ms: 1000,
          text: "main line",
          words: [
            { text: "main", time_ms: 1000, end_ms: 1500 },
            { text: "line", time_ms: 1500, end_ms: 2000 },
          ],
          bg_words: [
            { text: "bg", time_ms: 1200, end_ms: 1800 },
            { text: "vocal", time_ms: 1800, end_ms: 2000 },
          ],
          section: null,
        }}
        state="active"
        adjustedMs={1500}
        lyricsFontStep={0}
      />,
    );

    expect(markup).toContain("bg");
    expect(markup).toContain("vocal");
    expect(markup).toContain("translateY(0)");
  });

  test("renders bg_words hidden when line is future", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        line={{
          time_ms: 1000,
          text: "main line",
          words: [
            { text: "main", time_ms: 1000, end_ms: 1500 },
            { text: "line", time_ms: 1500, end_ms: 2000 },
          ],
          bg_words: [{ text: "bg", time_ms: 1200, end_ms: 1800 }],
          section: null,
        }}
        state="future"
        adjustedMs={0}
        lyricsFontStep={0}
      />,
    );

    expect(markup).toContain("translateY(8px)");
  });

  test("does not render duplicate main text for bg-only lines", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        line={{
          time_ms: 1000,
          text: "Back Up",
          words: null,
          bg_words: [
            { text: "Back", time_ms: 1000, end_ms: 1200 },
            { text: "Up", time_ms: 1200, end_ms: 1500 },
          ],
          section: null,
        }}
        state="active"
        adjustedMs={1200}
        lyricsFontStep={0}
      />,
    );

    expect(markup.match(/Back/g)).toHaveLength(1);
    expect(markup.match(/Up/g)).toHaveLength(1);
  });

  test("keeps active karaoke fill alpha contrast instead of collapsing the mask", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <LyricLine
          line={{
            time_ms: 1000,
            text: "alpha beta",
            words: [
              { text: "alpha", time_ms: 1000, end_ms: 1500 },
              { text: "beta", time_ms: 1500, end_ms: 2000 },
            ],
            bg_words: null,
            section: null,
          }}
          state="active"
          adjustedMs={1200}
          lyricsFontStep={0}
        />,
      );
    });

    expect(mockControllerInstances).toHaveLength(1);
    expect(mockControllerInstances[0].setTargetAlpha).toHaveBeenCalledWith(
      0.2,
      1.0,
    );
    expect(mockControllerInstances[0].setTargetAlpha).not.toHaveBeenCalledWith(
      1.0,
      1.0,
    );

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("keeps the karaoke controller when an active line becomes past", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const line = {
      time_ms: 1000,
      text: "alpha beta",
      words: [
        { text: "alpha", time_ms: 1000, end_ms: 1500 },
        { text: "beta", time_ms: 1500, end_ms: 2000 },
      ],
      bg_words: null,
      section: null,
    };

    await act(async () => {
      root.render(
        <LyricLine
          line={line}
          state="active"
          adjustedMs={1200}
          lyricsFontStep={0}
        />,
      );
    });
    const controller = mockControllerInstances[0];

    await act(async () => {
      root.render(
        <LyricLine
          line={line}
          state="past"
          adjustedMs={2500}
          lyricsFontStep={0}
        />,
      );
    });

    expect(mockControllerInstances).toHaveLength(1);
    expect(controller.destroy).not.toHaveBeenCalled();
    expect(controller.setCurrentAlpha).toHaveBeenLastCalledWith(1.0, 1.0);

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("pauses karaoke fill updates when playback is paused", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const line = {
      time_ms: 1000,
      text: "alpha beta",
      words: [
        { text: "alpha", time_ms: 1000, end_ms: 1500 },
        { text: "beta", time_ms: 1500, end_ms: 2000 },
      ],
      bg_words: null,
      section: null,
    };

    mockPlayerState.snapshot.is_playing = false;

    await act(async () => {
      root.render(
        <LyricLine
          line={line}
          state="active"
          adjustedMs={1200}
          lyricsFontStep={0}
        />,
      );
    });

    expect(mockControllerInstances[0].update).toHaveBeenLastCalledWith(
      1200,
      false,
    );

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("deactivates karaoke fill when an active line switches to audience presentation", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const line = {
      time_ms: 1000,
      text: "alpha beta",
      words: [
        { text: "alpha", time_ms: 1000, end_ms: 1500 },
        { text: "beta", time_ms: 1500, end_ms: 2000 },
      ],
      bg_words: null,
      section: null,
    };

    await act(async () => {
      root.render(
        <LyricLine
          line={line}
          state="active"
          adjustedMs={1200}
          lyricsFontStep={0}
        />,
      );
    });
    const controller = mockControllerInstances[0];

    await act(async () => {
      root.render(
        <LyricLine
          line={line}
          state="active"
          adjustedMs={1200}
          presentation="audience"
          lyricsFontStep={0}
        />,
      );
    });

    expect(controller.deactivateLine).toHaveBeenCalled();
    expect(controller.destroy).not.toHaveBeenCalled();

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("rebinds karaoke fill when emphasis rendering swaps word elements", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const line = {
      time_ms: 1000,
      text: "alpha beta",
      words: [
        { text: "alpha", time_ms: 1000, end_ms: 2500 },
        { text: "beta", time_ms: 2500, end_ms: 3000 },
      ],
      bg_words: null,
      section: null,
    };

    await act(async () => {
      root.render(
        <LyricLine
          line={line}
          state="active"
          adjustedMs={1200}
          lyricsFontStep={0}
        />,
      );
    });
    const controller = mockControllerInstances[0];

    await act(async () => {
      root.render(
        <LyricLine
          line={line}
          state="active"
          adjustedMs={2600}
          lyricsFontStep={0}
        />,
      );
    });

    expect(controller.activateLine).toHaveBeenCalledTimes(2);

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("keeps the karaoke controller when the same logical line gets a new object reference", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const makeLine = () => ({
      time_ms: 1000,
      text: "alpha beta",
      words: [
        { text: "alpha", time_ms: 1000, end_ms: 1500 },
        { text: "beta", time_ms: 1500, end_ms: 2000 },
      ],
      bg_words: null,
      section: null,
    });

    await act(async () => {
      root.render(
        <LyricLine
          line={makeLine()}
          state="active"
          adjustedMs={1200}
          lyricsFontStep={0}
        />,
      );
    });
    const firstController = mockControllerInstances[0];

    await act(async () => {
      root.render(
        <LyricLine
          line={makeLine()}
          state="active"
          adjustedMs={1300}
          lyricsFontStep={0}
        />,
      );
    });

    expect(mockControllerInstances).toHaveLength(1);
    expect(firstController.destroy).not.toHaveBeenCalled();

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("renders emphasis words as per-character spans with glow animation", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        line={{
          time_ms: 1000,
          text: "你好世界",
          words: [
            { text: "你好", time_ms: 1000, end_ms: 2500 },
            { text: "世界", time_ms: 2500, end_ms: 3000 },
          ],
          bg_words: null,
          section: null,
        }}
        state="active"
        adjustedMs={1200}
        lyricsFontStep={0}
      />,
    );

    // "你好" is active with duration 1500ms (>=1000) and CJK → emphasis
    expect(markup).toContain("lyric-char-glow");
    expect(markup).toContain("inline-block");
  });

  test("renders last word with amplified glow animation", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        line={{
          time_ms: 1000,
          text: "你好世界",
          words: [
            { text: "你好", time_ms: 1000, end_ms: 1500 },
            { text: "世界", time_ms: 1500, end_ms: 3000 },
          ],
          bg_words: null,
          section: null,
        }}
        state="active"
        adjustedMs={1600}
        lyricsFontStep={0}
      />,
    );

    // "世界" is active, last word, duration 1500ms, CJK → lyric-char-glow-last
    expect(markup).toContain("lyric-char-glow-last");
  });

  test("renders past line words with dimmer text color", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        line={{
          time_ms: 1000,
          text: "past line",
          words: [
            { text: "past", time_ms: 1000, end_ms: 1500 },
            { text: "line", time_ms: 1500, end_ms: 2000 },
          ],
          bg_words: null,
          section: null,
        }}
        state="past"
        adjustedMs={3000}
        lyricsFontStep={0}
      />,
    );

    expect(markup).toContain("text-white/45");
  });

  test("renders future line words with active text color", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        line={{
          time_ms: 1000,
          text: "future line",
          words: [
            { text: "future", time_ms: 1000, end_ms: 1500 },
            { text: "line", time_ms: 1500, end_ms: 2000 },
          ],
          bg_words: null,
          section: null,
        }}
        state="future"
        adjustedMs={0}
        lyricsFontStep={0}
      />,
    );

    expect(markup).toContain("text-white/50");
  });

  test("renders audience presentation with bg_words", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        line={{
          time_ms: 1000,
          text: "main",
          words: [{ text: "main", time_ms: 1000, end_ms: 1500 }],
          bg_words: [{ text: "bg", time_ms: 1200, end_ms: 1500 }],
          section: null,
        }}
        state="active"
        adjustedMs={1200}
        presentation="audience"
        lyricsFontStep={0}
      />,
    );

    expect(markup).toContain("bg");
    expect(markup).toContain("opacity:0.4");
  });

  test("renders non-emphasis active word with amplified glow as last word", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        line={{
          time_ms: 1000,
          text: "hello friend",
          words: [
            { text: "hello", time_ms: 1000, end_ms: 1200 },
            { text: "friend", time_ms: 1200, end_ms: 1400 },
          ],
          bg_words: null,
          section: null,
        }}
        state="active"
        adjustedMs={1300}
        lyricsFontStep={0}
      />,
    );

    // "friend" is active, last word → amplified glow (20px)
    expect(markup).toContain("text-white");
    expect(markup).toContain("0 0 20px");
  });
});
