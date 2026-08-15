// @vitest-environment jsdom

import { describe, expect, test, vi } from "vitest";
import { KaraokeFillController } from "@/components/Lyrics/karaoke-fill";
import { getLineVisualTargets, LyricsLineRuntime } from "./lyrics-line-runtime";

describe("getLineVisualTargets", () => {
  test("focus stage makes the current line dominate neighbors", () => {
    expect(getLineVisualTargets(0, "focus").targetScale).toBe(1);
    expect(getLineVisualTargets(1, "focus").targetScale).toBe(0.97);
    expect(getLineVisualTargets(0, "focus").targetOpacity).toBe(1);
    expect(getLineVisualTargets(1, "focus").targetBlur).toBeGreaterThan(0);
    expect(getLineVisualTargets(1, "focus").targetBlur).toBeLessThan(1);
    expect(getLineVisualTargets(0, "focus").targetBlur).toBe(0);
  });
});

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

  test("focus stage reserves absolute slots once lines have real height", () => {
    const runtime = new LyricsLineRuntime();
    const viewport = document.createElement("div");
    const stage = document.createElement("div");
    stage.dataset.lyricsStage = "focus";
    const first = document.createElement("div");
    const second = document.createElement("div");
    Object.defineProperty(first, "offsetHeight", { value: 40 });
    Object.defineProperty(second, "offsetHeight", { value: 20 });
    Object.defineProperty(viewport, "clientHeight", { value: 400 });
    stage.append(first, second);
    viewport.append(stage);
    runtime.registerWrapper(0, first);
    runtime.registerWrapper(1, second);

    runtime.tick({
      activeLineIndex: 0,
      adjustedMs: 0,
      isPlaying: true,
      dt: 0.05,
      isPlainText: false,
      stage: "focus",
      viewportEl: viewport,
    });

    expect(first.style.position).toBe("absolute");
    expect(second.style.position).toBe("absolute");
    expect(Number.parseFloat(second.style.top)).toBeGreaterThan(
      Number.parseFloat(first.style.top),
    );
    expect(stage.style.height).not.toBe("");

    runtime.clear();
    expect(stage.style.height).toBe("");
    expect(first.style.position).toBe("");
    expect(second.style.top).toBe("");
  });

  test("focus stage scales from the center of the line", () => {
    const runtime = new LyricsLineRuntime();
    const wrapper = document.createElement("div");
    runtime.registerWrapper(0, wrapper);

    runtime.tick({
      activeLineIndex: 0,
      adjustedMs: 0,
      isPlaying: true,
      dt: 0.05,
      isPlainText: false,
      stage: "focus",
    });

    expect(wrapper.style.transformOrigin).toBe("center center");
  });

  test("skips plain-text ticks and wrappers without a DOM node", () => {
    const runtime = new LyricsLineRuntime();
    const wrapper = document.createElement("div");
    runtime.registerWrapper(0, wrapper);
    runtime.unregisterWrapper(0);

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

    const settledBlur = wrapper.style.filter;
    expect(settledBlur).toContain("blur(");

    runtime.unregisterWrapper(0);
    runtime.registerWrapper(0, wrapper);
    runtime.tick({
      activeLineIndex: 5,
      adjustedMs: 0,
      isPlaying: true,
      dt: 0.016,
      isPlainText: false,
    });

    expect(wrapper.style.filter).toBe(settledBlur);
  });
});
