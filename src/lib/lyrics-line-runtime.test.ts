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
});
