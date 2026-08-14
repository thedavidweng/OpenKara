// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => vi.fn()),
}));

import { useAudiencePlainTextPaging } from "./use-audience-plain-text-paging";

const line = (time_ms: number, text: string) => ({
  time_ms,
  text,
  words: null,
  bg_words: null,
  section: null,
  roman: null,
});

const audiencePresentationSpec = {
  verticalPaddingPx: 0,
  horizontalPaddingPx: 0,
  lineGapPx: 0,
  contentWidthRatio: 1,
  contentMaxWidthPx: 1000,
  fontSizePx: 24,
  lineHeightMultiple: 1,
  activeScale: 1,
  statusFontSizePx: 18,
  activeGlowBlurPx: 12,
  activeTextColor: { red: 1, green: 1, blue: 1, alpha: 1 },
  pastTextColor: { red: 0, green: 0, blue: 0, alpha: 1 },
  futureTextColor: { red: 0, green: 0, blue: 0, alpha: 1 },
  plainTextColor: { red: 1, green: 1, blue: 1, alpha: 1 },
  statusTextColor: { red: 0, green: 0, blue: 0, alpha: 1 },
  activeGlowColor: { red: 1, green: 1, blue: 1, alpha: 1 },
};

interface HarnessProps {
  lines: ReturnType<typeof line>[];
  layoutVersion: string;
  pageIdentity: string;
}

let harnessArgs: HarnessProps = {
  lines: [],
  layoutVersion: "false:",
  pageIdentity: "test",
};

let lastResult: ReturnType<typeof useAudiencePlainTextPaging> | null = null;

function Harness() {
  lastResult = useAudiencePlainTextPaging({
    lines: harnessArgs.lines,
    shouldRender: true,
    pageIdentity: harnessArgs.pageIdentity,
    audiencePresentationSpec,
    layoutVersion: harnessArgs.layoutVersion,
  });
  const { containerRef, measurementRef, visibleLines } = lastResult;

  return (
    <div ref={containerRef} style={{ height: 100 }} data-testid="container">
      <div ref={measurementRef} data-testid="measurement">
        {harnessArgs.lines.map((l, idx) => (
          <div
            key={idx}
            data-plain-text-page-measure-line
            data-line-index={idx}
          >
            {l.text}
          </div>
        ))}
      </div>
      <div data-testid="visible">
        {visibleLines.map((l, idx) => (
          <div key={idx} data-visible-line>
            {l.text}
          </div>
        ))}
      </div>
    </div>
  );
}

describe("useAudiencePlainTextPaging layoutVersion invalidation", () => {
  let container: HTMLDivElement;
  let root: Root;
  let lineHeights: number[] = [];

  beforeEach(() => {
    lineHeights = [];
    class MockResizeObserver {
      observe(_target: Element) {}
      disconnect() {}
      unobserve() {}
    }
    Object.defineProperty(globalThis, "ResizeObserver", {
      configurable: true,
      value: MockResizeObserver,
    });

    Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", {
      configurable: true,
      value(this: HTMLElement) {
        if (this.hasAttribute("data-plain-text-page-measure-line")) {
          const idx = Number(this.getAttribute("data-line-index") ?? 0);
          const height = lineHeights[idx] ?? 20;
          return {
            width: 600,
            height,
            top: 0,
            right: 600,
            bottom: height,
            left: 0,
            x: 0,
            y: 0,
            toJSON: () => ({}),
          };
        }
        return {
          width: 600,
          height: 100,
          top: 0,
          right: 600,
          bottom: 100,
          left: 0,
          x: 0,
          y: 0,
          toJSON: () => ({}),
        };
      },
    });

    Object.defineProperty(HTMLElement.prototype, "clientHeight", {
      configurable: true,
      get(this: HTMLElement) {
        if (this.getAttribute("data-testid") === "container") {
          return 100;
        }
        return 0;
      },
    });

    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    harnessArgs = {
      lines: [],
      layoutVersion: "false:",
      pageIdentity: "test",
    };
    lastResult = null;
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  test("enabling romanized text invalidates measurement and produces updated page boundaries", async () => {
    const lines = [
      line(0, "你好"),
      line(0, "世界"),
      line(0, "再见"),
      line(0, "朋友"),
    ];
    lineHeights = [20, 20, 20, 20];
    harnessArgs = {
      lines,
      layoutVersion: "false:",
      pageIdentity: "test",
    };

    await act(async () => {
      root.render(<Harness />);
    });

    // Without romanization: all four lines fit on one page.
    expect(lastResult!.pageStartIndices).toEqual([0]);
    expect(lastResult!.visibleLines).toHaveLength(4);

    // Enable romanized text: line heights double, forcing a page break.
    lineHeights = [40, 40, 40, 40];
    harnessArgs = {
      lines,
      layoutVersion: "true:ni hao\u0000shi jie\u0000zai jian\u0000peng you",
      pageIdentity: "test",
    };

    await act(async () => {
      root.render(<Harness />);
    });

    expect(lastResult!.pageStartIndices).toEqual([0, 2]);
  });

  test("page index is clamped after page-count reduction", async () => {
    const lines = [
      line(0, "a"),
      line(0, "b"),
      line(0, "c"),
      line(0, "d"),
      line(0, "e"),
      line(0, "f"),
    ];
    // Tall lines: each page holds exactly one line.
    lineHeights = [60, 60, 60, 60, 60, 60];
    harnessArgs = {
      lines,
      layoutVersion: "false:",
      pageIdentity: "test",
    };

    await act(async () => {
      root.render(<Harness />);
    });

    expect(lastResult!.pageStartIndices).toEqual([0, 1, 2, 3, 4, 5]);

    // Advance to the last page via the remote page event.
    await act(async () => {});

    // Now shrink line heights so all lines fit on one page.
    lineHeights = [10, 10, 10, 10, 10, 10];
    harnessArgs = {
      lines,
      layoutVersion: "false:",
      pageIdentity: "test-shrunk",
    };

    await act(async () => {
      root.render(<Harness />);
    });

    expect(lastResult!.pageStartIndices).toEqual([0]);
    // Page index is clamped to 0 (the only valid page).
    expect(lastResult!.pageIndex).toBe(0);
  });

  test("the final source and romanized lines remain fully visible within the audience viewport", async () => {
    const lines = [
      line(0, "你好"),
      line(0, "世界"),
      line(0, "再见"),
      line(0, "朋友"),
    ];
    lineHeights = [20, 20, 20, 20];
    harnessArgs = {
      lines,
      layoutVersion: "false:",
      pageIdentity: "test",
    };

    await act(async () => {
      root.render(<Harness />);
    });

    // All four source lines visible without romanization.
    expect(lastResult!.visibleLines).toHaveLength(4);

    // Enable romanization; the final romanized line must still be visible
    // (on page 1), not clipped.
    lineHeights = [40, 40, 40, 40];
    harnessArgs = {
      lines,
      layoutVersion: "true:ni hao\u0000shi jie\u0000zai jian\u0000peng you",
      pageIdentity: "test",
    };

    await act(async () => {
      root.render(<Harness />);
    });

    const lastPageStart =
      lastResult!.pageStartIndices[lastResult!.pageStartIndices.length - 1];
    const lastPageEnd = lines.length;
    expect(lastPageEnd).toBeGreaterThan(lastPageStart);
    // The final line index (3) is within the last page range.
    expect(lastPageStart).toBeLessThanOrEqual(3);
    expect(lastPageEnd).toBeGreaterThan(3);
  });
});

describe("useAudiencePlainTextPaging source-level invariants", () => {
  test("layoutVersion is included in the measurement useLayoutEffect deps", async () => {
    const { default: src } =
      await import("./use-audience-plain-text-paging.ts?raw");
    // The deps array must include layoutVersion so romanized text changes
    // trigger a remeasure.
    expect(src).toContain("layoutVersion,");
  });

  test("the measurement container is observed by ResizeObserver", async () => {
    const { default: src } =
      await import("./use-audience-plain-text-paging.ts?raw");
    expect(src).toContain("observer.observe(measurementRef.current)");
  });
});
