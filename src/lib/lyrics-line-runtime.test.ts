// @vitest-environment jsdom

import { describe, expect, test, vi } from "vitest";
import { KaraokeFillController } from "@/components/Lyrics/karaoke-fill";
import { LyricsLineRuntime } from "./lyrics-line-runtime";

describe("LyricsLineRuntime", () => {
  test("updates karaoke fill from the unified engine tick", () => {
    const runtime = new LyricsLineRuntime();
    const wrapper = document.createElement("div");
    runtime.registerWrapper(0, wrapper);

    const karaoke = {
      update: vi.fn(),
    } as unknown as KaraokeFillController;
    runtime.registerKaraoke(0, karaoke);

    runtime.tick({
      activeLineIndex: 0,
      adjustedMs: 1200,
      isPlaying: false,
      dt: 0.016,
      isPlainText: false,
    });

    expect(karaoke.update).toHaveBeenCalledWith(1200, false);
  });

  test("applies line spring transforms directly to registered wrappers", () => {
    const runtime = new LyricsLineRuntime();
    const wrapper = document.createElement("div");
    runtime.registerWrapper(0, wrapper);
    runtime.registerWrapper(1, wrapper);

    runtime.tick({
      activeLineIndex: 1,
      adjustedMs: 2000,
      isPlaying: true,
      dt: 0.05,
      isPlainText: false,
    });

    expect(wrapper.style.transform).toContain("scale(");
    expect(wrapper.style.opacity).not.toBe("");
  });

  test("skips plain-text ticks and wrappers without a DOM node", () => {
    const runtime = new LyricsLineRuntime();
    const wrapper = document.createElement("div");
    runtime.registerWrapper(0, wrapper);
    runtime.unregisterWrapper(0); // leaves springs, clears wrapperEl

    runtime.tick({
      activeLineIndex: 0,
      adjustedMs: 0,
      isPlaying: true,
      dt: 0.016,
      isPlainText: true,
    });
    expect(wrapper.style.transform).toBe("");

    runtime.tick({
      activeLineIndex: 0,
      adjustedMs: 0,
      isPlaying: true,
      dt: 0.016,
      isPlainText: false,
    });
    expect(wrapper.style.transform).toBe("");
  });

  test("re-registering an existing wrapper only updates the element pointer", () => {
    const runtime = new LyricsLineRuntime();
    const first = document.createElement("div");
    const second = document.createElement("div");
    runtime.registerWrapper(0, first);
    runtime.registerWrapper(0, second);

    runtime.tick({
      activeLineIndex: 0,
      adjustedMs: 0,
      isPlaying: true,
      dt: 0.05,
      isPlainText: false,
    });

    expect(second.style.transform).toContain("scale(");
    expect(first.style.transform).toBe("");
  });

  test("preserves spring progress across unregister/register churn", () => {
    const runtime = new LyricsLineRuntime();
    const wrapper = document.createElement("div");
    runtime.registerWrapper(0, wrapper);

    for (let i = 0; i < 40; i++) {
      runtime.tick({
        activeLineIndex: 5,
        adjustedMs: 0,
        isPlaying: true,
        dt: 0.05,
        isPlainText: false,
      });
    }

    const settledOpacity = wrapper.style.opacity;
    expect(Number(settledOpacity)).toBeLessThan(0.95);

    runtime.unregisterWrapper(0);
    runtime.registerWrapper(0, wrapper);
    runtime.tick({
      activeLineIndex: 5,
      adjustedMs: 0,
      isPlaying: true,
      dt: 0.016,
      isPlainText: false,
    });

    expect(wrapper.style.opacity).toBe(settledOpacity);
  });
});
