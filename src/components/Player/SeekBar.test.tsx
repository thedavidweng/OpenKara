// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { SeekBar } from "./SeekBar";

const { mockPlayerState, mockSeek, mockSelectCurrentPositionMs } = vi.hoisted(
  () => {
    const mockSeek = vi.fn();
    const mockSelectCurrentPositionMs = vi.fn(
      (state: { positionMs: number }) => state.positionMs,
    );
    return {
      mockSeek,
      mockSelectCurrentPositionMs,
      mockPlayerState: {
        snapshot: {
          duration_ms: 100_000,
          is_playing: true,
          state: "playing",
        },
        positionMs: 10_000,
        playingSinceMs: 1000,
        seek: mockSeek,
      },
    };
  },
);

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: (selector: (state: typeof mockPlayerState) => unknown) =>
    selector(mockPlayerState),
  selectCurrentPositionMs: mockSelectCurrentPositionMs,
}));

describe("SeekBar", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    mockSeek.mockClear();
    mockPlayerState.positionMs = 10_000;
    mockSelectCurrentPositionMs.mockImplementation(
      (state: { positionMs: number }) => state.positionMs,
    );
    // Do not invoke the callback synchronously — SeekBar chains rAF forever.
    vi.stubGlobal(
      "requestAnimationFrame",
      vi.fn(() => 1),
    );
    vi.stubGlobal("cancelAnimationFrame", vi.fn());

    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    host.remove();
    vi.unstubAllGlobals();
  });

  test("keeps a minimum safe width for the whole control and the draggable rail", async () => {
    const { renderToStaticMarkup } = await import("react-dom/server");
    const markup = renderToStaticMarkup(<SeekBar density="tight" />);

    expect(markup).toContain("min-w-[180px]");
    expect(markup).toContain("min-w-[120px]");
    expect(markup).toContain("w-[3.25rem]");
    expect(markup).toContain("font-[tabular-nums]");
    expect(markup).toContain("whitespace-nowrap");
  });

  test("mouseup after drag seeks to the release position", () => {
    act(() => {
      root.render(<SeekBar />);
    });

    const rail = host.querySelector("[role='slider']") as HTMLElement;
    expect(rail).toBeTruthy();

    // jsdom layout is zero-sized by default; stub the rail geometry.
    vi.spyOn(rail, "getBoundingClientRect").mockReturnValue({
      left: 0,
      width: 200,
      top: 0,
      height: 10,
      right: 200,
      bottom: 10,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });

    act(() => {
      rail.dispatchEvent(
        new MouseEvent("mousedown", { clientX: 50, bubbles: true }),
      );
    });
    act(() => {
      window.dispatchEvent(
        new MouseEvent("mouseup", { clientX: 100, bubbles: true }),
      );
    });

    // 100/200 = 50% of 100_000ms
    expect(mockSeek).toHaveBeenCalledWith(50_000);
  });
});
