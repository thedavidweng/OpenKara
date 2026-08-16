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
  INACTIVE_MASK_ALPHA: 0.2,
  ACTIVE_BRIGHT_ALPHA: 1,
  ACTIVE_DARK_ALPHA: 0.4,
  applyWordMask: vi.fn(() => ({ width: 80, fade: 20 })),
  KaraokeFillController: vi.fn().mockImplementation(function () {
    const controller = {
      activateLine: vi.fn(),
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
          roman: null,
        }}
        lineIndex={0}
        state="plain"
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
          roman: null,
        }}
        lineIndex={0}
        state="future"
        lyricsFontStep={0}
      />,
    );

    expect(markup).toContain("cursor-pointer");
    expect(markup).toContain("group-hover/line:-translate-y-px");
    expect(markup).toContain("group-hover/line:[text-shadow:");
    expect(markup).not.toContain("group-hover/line:bg-white/10");
  });

  test("clicking a seekable line seeks to the line time without pre-arming lyrics isSeek", () => {
    mockSeek.mockClear();
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);

    act(() => {
      root.render(
        <LyricLine
          line={{
            time_ms: 15_000,
            text: "jump here",
            words: null,
            bg_words: null,
            section: null,
            roman: null,
          }}
          lineIndex={2}
          state="future"
          lyricsFontStep={0}
        />,
      );
    });

    const clickable = host.querySelector(
      ".cursor-pointer",
    ) as HTMLButtonElement;
    expect(clickable).toBeTruthy();
    expect(clickable.tagName).toBe("BUTTON");
    act(() => {
      clickable.click();
    });
    expect(mockSeek).toHaveBeenCalledWith(15_000);

    act(() => {
      root.unmount();
    });
    host.remove();
  });

  test("renders word-level states for the active line without changing lyric timing behavior", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
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
          roman: null,
        }}
        state="active"
        activeWordIndex={1}
        lyricsFontStep={0}
      />,
    );

    expect(markup).toContain("text-[var(--color-lyrics-active)]");
    expect(markup).toContain('data-karaoke-fill="true"');
    expect(markup).toContain("--bright-mask-alpha");
    expect(markup).toContain("--dark-mask-alpha");
  });

  test("uses the configured font scale without changing the lyric state logic", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={{
          time_ms: 1000,
          text: "scaled line",
          words: null,
          bg_words: null,
          section: null,
          roman: null,
        }}
        state="active"
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
        lineIndex={0}
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
          roman: null,
        }}
        state="active"
        activeWordIndex={1}
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
          roman: null,
        }}
        lineIndex={0}
        state="future"
        lyricsFontStep={0}
      />,
    );

    expect(markup).toContain("height:0");
    expect(markup).toContain("visibility:hidden");
  });

  test("does not render duplicate main text for bg-only lines", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={{
          time_ms: 1000,
          text: "Back Up",
          words: null,
          bg_words: [
            { text: "Back", time_ms: 1000, end_ms: 1200 },
            { text: "Up", time_ms: 1200, end_ms: 1500 },
          ],
          section: null,
          roman: null,
        }}
        state="active"
        lyricsFontStep={0}
      />,
    );

    expect(markup.match(/Back/g)).toHaveLength(1);
    expect(markup.match(/Up/g)).toHaveLength(1);
  });

  test("keeps dim word text visible under the highlight overlay on the active line", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <LyricLine
          lineIndex={0}
          line={{
            time_ms: 1000,
            text: "alpha beta",
            words: [
              { text: "alpha", time_ms: 1000, end_ms: 1500 },
              { text: "beta", time_ms: 1500, end_ms: 2000 },
            ],
            bg_words: null,
            section: null,
            roman: null,
          }}
          state="active"
          lyricsFontStep={0}
        />,
      );
    });

    expect(mockControllerInstances).toHaveLength(1);
    expect(mockControllerInstances[0].activateLine).toHaveBeenCalled();
    expect(container.querySelectorAll("[data-karaoke-fill]")).toHaveLength(2);

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("deactivates karaoke fill when an active line becomes past", async () => {
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
      roman: null,
    };

    await act(async () => {
      root.render(
        <LyricLine
          lineIndex={0}
          line={line}
          state="active"
          lyricsFontStep={0}
        />,
      );
    });
    const controller = mockControllerInstances[0];

    await act(async () => {
      root.render(
        <LyricLine lineIndex={0} line={line} state="past" lyricsFontStep={0} />,
      );
    });

    expect(mockControllerInstances).toHaveLength(1);
    expect(controller.deactivateLine).toHaveBeenCalled();
    expect(controller.destroy).not.toHaveBeenCalled();
    expect(container.querySelectorAll("[data-karaoke-fill]")).toHaveLength(0);
    expect(container.innerHTML).toContain("text-[var(--color-lyrics-past)]");
    expect(container.innerHTML).not.toContain("--bright-mask-alpha");

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
      roman: null,
    };

    await act(async () => {
      root.render(
        <LyricLine
          lineIndex={0}
          line={line}
          state="active"
          lyricsFontStep={0}
        />,
      );
    });
    const controller = mockControllerInstances[0];

    await act(async () => {
      root.render(
        <LyricLine
          lineIndex={0}
          line={line}
          state="active"
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

  test("does not rebuild karaoke fill when only the active word index changes", async () => {
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
      roman: null,
    };

    await act(async () => {
      root.render(
        <LyricLine
          lineIndex={0}
          line={line}
          state="active"
          activeWordIndex={0}
          lyricsFontStep={0}
        />,
      );
    });
    const controller = mockControllerInstances[0];
    const activateCount = controller.activateLine.mock.calls.length;

    await act(async () => {
      root.render(
        <LyricLine
          lineIndex={0}
          line={line}
          state="active"
          activeWordIndex={1}
          lyricsFontStep={0}
        />,
      );
    });

    expect(controller.activateLine.mock.calls.length).toBe(activateCount);

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("rebinds karaoke fill when word timings change without romanized text changing", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const makeLine = (startMs: number) => ({
      time_ms: startMs,
      text: "君の",
      words: [
        { text: "君", time_ms: startMs, end_ms: startMs + 500, roman: "kimi" },
        {
          text: "の",
          time_ms: startMs + 500,
          end_ms: startMs + 1000,
          roman: "no",
        },
      ],
      bg_words: null,
      section: null,
      roman: "kimi no",
    });

    await act(async () => {
      root.render(
        <LyricLine
          lineIndex={0}
          line={makeLine(1000)}
          state="active"
          lyricsFontStep={0}
          alignment="left"
          romanizedText="kimi no"
        />,
      );
    });
    const controller = mockControllerInstances[0];
    const activateCount = controller.activateLine.mock.calls.length;

    await act(async () => {
      root.render(
        <LyricLine
          lineIndex={0}
          line={makeLine(4000)}
          state="active"
          lyricsFontStep={0}
          alignment="left"
          romanizedText="kimi no"
        />,
      );
    });

    expect(controller.activateLine.mock.calls.length).toBeGreaterThan(
      activateCount,
    );
    const lastCall =
      controller.activateLine.mock.calls[
        controller.activateLine.mock.calls.length - 1
      ];
    expect(lastCall?.[1]?.[0]?.time_ms).toBe(4000);

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  test("binds roman karaoke fills when pronunciation is turned on mid-line", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const line = {
      time_ms: 1000,
      text: "君の",
      words: [
        { text: "君", time_ms: 1000, end_ms: 1500, roman: "kimi" },
        { text: "の", time_ms: 1500, end_ms: 2000, roman: "no" },
      ],
      bg_words: null,
      section: null,
      roman: "kimi no",
    };

    await act(async () => {
      root.render(
        <LyricLine
          lineIndex={0}
          line={line}
          state="active"
          lyricsFontStep={0}
          alignment="left"
        />,
      );
    });
    const controller = mockControllerInstances[0];
    const activateCount = controller.activateLine.mock.calls.length;

    await act(async () => {
      root.render(
        <LyricLine
          lineIndex={0}
          line={line}
          state="active"
          lyricsFontStep={0}
          alignment="left"
          romanizedText="kimi no"
        />,
      );
    });

    expect(controller.activateLine.mock.calls.length).toBeGreaterThan(
      activateCount,
    );
    const lastCall =
      controller.activateLine.mock.calls[
        controller.activateLine.mock.calls.length - 1
      ];
    expect(lastCall?.[4]).toHaveLength(2);

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
      roman: null,
    });

    await act(async () => {
      root.render(
        <LyricLine
          lineIndex={0}
          line={makeLine()}
          state="active"
          lyricsFontStep={0}
        />,
      );
    });
    const firstController = mockControllerInstances[0];

    await act(async () => {
      root.render(
        <LyricLine
          lineIndex={0}
          line={makeLine()}
          state="active"
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

  test("renders emphasis words with a highlight overlay instead of hiding the base text", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={{
          time_ms: 1000,
          text: "你好世界",
          words: [
            { text: "你好", time_ms: 1000, end_ms: 2500 },
            { text: "世界", time_ms: 2500, end_ms: 3000 },
          ],
          bg_words: null,
          section: null,
          roman: null,
        }}
        state="active"
        activeWordIndex={0}
        lyricsFontStep={0}
      />,
    );

    expect(markup).toContain('data-karaoke-fill="true"');
    expect(markup).toContain("lyric-word-emphasize");
    expect(markup).not.toContain("lyric-char-glow");
  });

  test("renders last word with amplified glow on the highlight overlay", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={{
          time_ms: 1000,
          text: "你好世界",
          words: [
            { text: "你好", time_ms: 1000, end_ms: 1500 },
            { text: "世界", time_ms: 1500, end_ms: 3000 },
          ],
          bg_words: null,
          section: null,
          roman: null,
        }}
        state="active"
        activeWordIndex={1}
        lyricsFontStep={0}
      />,
    );

    expect(markup).toContain('data-karaoke-fill="true"');
    expect(markup).toContain("lyric-word-emphasize-last");
  });

  test("renders past line words with dimmer text color", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={{
          time_ms: 1000,
          text: "past line",
          words: [
            { text: "past", time_ms: 1000, end_ms: 1500 },
            { text: "line", time_ms: 1500, end_ms: 2000 },
          ],
          bg_words: null,
          section: null,
          roman: null,
        }}
        state="past"
        lyricsFontStep={0}
      />,
    );

    expect(markup).toContain("text-[var(--color-lyrics-past)]");
    expect(markup).not.toContain("data-karaoke-fill");
    expect(markup).not.toContain("--bright-mask-alpha");
  });

  test("renders future line words with future text color", () => {
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
          roman: null,
        }}
        lineIndex={0}
        state="future"
        lyricsFontStep={0}
      />,
    );

    expect(markup).toContain("text-[var(--color-lyrics-future)]");
    expect(markup).not.toContain("data-karaoke-fill");
    expect(markup).not.toContain("--bright-mask-alpha");
  });

  test("renders audience presentation with bg_words", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={{
          time_ms: 1000,
          text: "main",
          words: [{ text: "main", time_ms: 1000, end_ms: 1500 }],
          bg_words: [{ text: "bg", time_ms: 1200, end_ms: 1500 }],
          section: null,
          roman: null,
        }}
        state="active"
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
        lineIndex={0}
        line={{
          time_ms: 1000,
          text: "hello friend",
          words: [
            { text: "hello", time_ms: 1000, end_ms: 1200 },
            { text: "friend", time_ms: 1200, end_ms: 1400 },
          ],
          bg_words: null,
          section: null,
          roman: null,
        }}
        state="active"
        activeWordIndex={1}
        lyricsFontStep={0}
      />,
    );

    expect(markup).toContain("text-[var(--color-lyrics-active)]");
    expect(markup).toContain('data-karaoke-fill="true"');
  });

  test("scales centered roman with the line viewport size, not a rem step on the button", () => {
    const line = {
      time_ms: 1000,
      text: "你好世界",
      words: [
        { text: "你好", time_ms: 1000, end_ms: 1500 },
        { text: "世界", time_ms: 1500, end_ms: 2000 },
      ],
      bg_words: null,
      section: null,
      roman: null,
    };

    const smallStep = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={line}
        state="active"
        lyricsFontStep={-2}
        romanizedText="ni hao shi jie"
        alignment="center"
      />,
    );
    const largeStep = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={line}
        state="active"
        lyricsFontStep={2}
        romanizedText="ni hao shi jie"
        alignment="center"
      />,
    );

    expect(smallStep).toContain("clamp(2.15rem, 1.7vw + 1.8vh, 3rem) * 0.76");
    expect(largeStep).toContain("clamp(2.15rem, 1.7vw + 1.8vh, 3rem) * 1.28");
    expect(smallStep).toContain("max(0.5em, 10px)");
    expect(smallStep).toContain('data-word-roman="true"');
    expect(smallStep).toContain("ni hao");
    expect(smallStep).toContain("shi jie");
    expect(smallStep).not.toContain("text-[0.5em]");
  });

  test("keeps centered type metrics stable so highlight does not reflow the line", () => {
    const line = {
      time_ms: 34000,
      text: "'Cause you make my earthquake (Earthquake)",
      words: null,
      bg_words: null,
      section: null,
      roman: null,
    };

    const active = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={line}
        state="active"
        lyricsFontStep={0}
        romanizedText="'Cause you make my earthquake (Earthquake)"
        alignment="center"
      />,
    );
    const past = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={line}
        state="past"
        lyricsFontStep={0}
        romanizedText="'Cause you make my earthquake (Earthquake)"
        alignment="center"
      />,
    );

    expect(active).toContain("font-weight:600");
    expect(past).toContain("font-weight:600");
    expect(active).not.toContain("font-weight:500");
    expect(past).not.toContain("font-weight:500");
    expect(active).toContain("clamp(2.15rem, 1.7vw + 1.8vh, 3rem)");
    expect(past).toContain("clamp(2.15rem, 1.7vw + 1.8vh, 3rem)");
  });

  test("scales left-aligned roman with lyricsFontStep in standard mode", () => {
    const line = {
      time_ms: 1000,
      text: "你好世界",
      words: [
        { text: "你好", time_ms: 1000, end_ms: 1500 },
        { text: "世界", time_ms: 1500, end_ms: 2000 },
      ],
      bg_words: null,
      section: null,
      roman: null,
    };

    const smallStep = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={line}
        state="active"
        lyricsFontStep={-2}
        romanizedText="ni hao shi jie"
        alignment="left"
      />,
    );
    const largeStep = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={line}
        state="active"
        lyricsFontStep={2}
        romanizedText="ni hao shi jie"
        alignment="left"
      />,
    );

    expect(smallStep).toContain("text-base");
    expect(largeStep).toContain("text-3xl");
    expect(largeStep).toContain("xl:text-5xl");
  });

  test("keeps centered roman under the main line and above background words", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={{
          time_ms: 1000,
          text: "忘れられない人",
          words: [{ text: "忘れられない人", time_ms: 1000, end_ms: 2000 }],
          bg_words: [
            {
              text: "(I love you more than you'll ever know)",
              time_ms: 1000,
              end_ms: 2000,
            },
          ],
          section: null,
          roman: "wasurerarenai hito",
        }}
        state="active"
        lyricsFontStep={0}
        romanizedText="wasurerarenai hito"
        alignment="center"
      />,
    );

    const romanAt = markup.indexOf("data-word-roman");
    const bgAt = markup.indexOf("I love you more");
    const mainAt = markup.indexOf("忘れられない人");
    expect(romanAt).toBeGreaterThan(mainAt);
    expect(bgAt).toBeGreaterThan(romanAt);
    expect(markup).toContain("wasurerarenai hito");
    expect(markup).toContain("max(0.5em, 10px)");
    expect(markup).toContain("max(0.7em, 10px)");
    expect(markup).toContain('data-lyrics-bg="true"');
    expect(markup).not.toContain("text-[0.5em]");
  });

  test("does not echo English lyrics as a second romanization row", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={{
          time_ms: 1000,
          text: "'Cause you make my earthquake (Earthquake)",
          words: null,
          bg_words: null,
          section: null,
          roman: "'Cause you make my earthquake (Earthquake)",
        }}
        state="active"
        lyricsFontStep={0}
        romanizedText="'Cause you make my earthquake (Earthquake)"
        alignment="center"
      />,
    );

    expect(markup).toContain("Cause you make my earthquake (Earthquake)");
    expect(markup).not.toContain('data-lyrics-roman="true"');
    expect(markup).not.toContain('data-word-roman="true"');
  });

  test("puts left-aligned roman in the right column even when word romans resolve", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={{
          time_ms: 1000,
          text: "君の",
          words: [
            { text: "君", time_ms: 1000, end_ms: 1500, roman: "kimi" },
            { text: "の", time_ms: 1500, end_ms: 2000, roman: "no" },
          ],
          bg_words: null,
          section: null,
          roman: "kimi no",
        }}
        state="active"
        lyricsFontStep={0}
        romanizedText="kimi no"
        alignment="left"
      />,
    );

    expect(markup).toContain("minmax(14rem,18rem)");
    expect(markup).toContain('data-lyrics-roman="true"');
    expect(markup).toContain("kimi");
    expect(markup).toContain("no");
    expect(markup).toContain('data-karaoke-roman-fill="true"');
    expect(markup).not.toContain('data-word-roman="true"');
  });

  test("keeps left-aligned original and roman on the same line highlight", () => {
    const line = {
      time_ms: 1000,
      text: "君の",
      words: [
        { text: "君", time_ms: 1000, end_ms: 1500, roman: "kimi" },
        { text: "の", time_ms: 1500, end_ms: 2000, roman: "no" },
      ],
      bg_words: null,
      section: null,
      roman: "kimi no",
    };

    const past = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={line}
        state="past"
        lyricsFontStep={0}
        romanizedText="kimi no"
        alignment="left"
      />,
    );
    const future = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={line}
        state="future"
        lyricsFontStep={0}
        romanizedText="kimi no"
        alignment="left"
      />,
    );

    expect(past.match(/text-\[var\(--color-lyrics-past\)\]/g)?.length).toBe(3);
    expect(past).toMatch(
      /data-lyrics-roman="true"[^>]*text-\[var\(--color-lyrics-past\)\]/,
    );
    expect(past).not.toMatch(
      /(?<!group-hover\/line:)text-\[var\(--color-lyrics-active\)\]/,
    );
    expect(past).not.toContain("text-[var(--color-lyrics-future)]");
    expect(future.match(/text-\[var\(--color-lyrics-future\)\]/g)?.length).toBe(
      3,
    );
    expect(future).toMatch(
      /data-lyrics-roman="true"[^>]*text-\[var\(--color-lyrics-future\)\]/,
    );
    expect(future).not.toMatch(
      /(?<!group-hover\/line:)text-\[var\(--color-lyrics-active\)\]/,
    );
    expect(future).not.toContain("text-[var(--color-lyrics-past)]");
  });

  test("puts aligned roman under each word and hides the line sub-row", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={{
          time_ms: 1000,
          text: "君の",
          words: [
            { text: "君", time_ms: 1000, end_ms: 1500, roman: "kimi" },
            { text: "の", time_ms: 1500, end_ms: 2000, roman: "no" },
          ],
          bg_words: [
            {
              text: "(harmony)",
              time_ms: 1000,
              end_ms: 2000,
            },
          ],
          section: null,
          roman: "kimi no",
        }}
        state="active"
        lyricsFontStep={0}
        romanizedText="kimi no"
        alignment="center"
      />,
    );

    expect(markup).toContain('data-word-roman="true"');
    expect(markup).toMatch(/data-word-roman="true"[^>]*>kimi</);
    expect(markup).toMatch(/data-word-roman="true"[^>]*>no</);
    expect(markup).toContain('data-karaoke-roman-fill="true"');
    expect(markup).not.toContain('data-lyrics-roman="true"');
    expect(markup.indexOf("kimi")).toBeLessThan(markup.indexOf("(harmony)"));
  });

  test("does not render supplied word roman until romanized text is enabled", () => {
    const markup = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={{
          time_ms: 1000,
          text: "君",
          words: [{ text: "君", time_ms: 1000, end_ms: 1500, roman: "kimi" }],
          bg_words: null,
          section: null,
          roman: "kimi",
        }}
        state="active"
        lyricsFontStep={0}
        alignment="center"
      />,
    );

    expect(markup).not.toContain('data-word-roman="true"');
    expect(markup).not.toContain('data-lyrics-roman="true"');
  });

  test("scales left-aligned bg_words with lyricsFontStep in standard mode", () => {
    const line = {
      time_ms: 1000,
      text: "main line",
      words: [
        { text: "main", time_ms: 1000, end_ms: 1500 },
        { text: "line", time_ms: 1500, end_ms: 2000 },
      ],
      bg_words: [{ text: "bg", time_ms: 1200, end_ms: 1800 }],
      section: null,
      roman: null,
    };

    const smallStep = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={line}
        state="active"
        activeWordIndex={1}
        lyricsFontStep={-2}
        alignment="left"
      />,
    );
    const largeStep = renderToStaticMarkup(
      <LyricLine
        lineIndex={0}
        line={line}
        state="active"
        activeWordIndex={1}
        lyricsFontStep={2}
        alignment="left"
      />,
    );

    expect(smallStep).toContain("text-xs");
    expect(largeStep).toContain("text-lg");
    expect(largeStep).toContain("xl:text-3xl");
  });
});
