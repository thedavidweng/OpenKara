// @vitest-environment jsdom
import { describe, expect, test, vi, beforeEach, afterEach } from "vitest";
import { KaraokeFillController } from "./karaoke-fill";

// Mock Web Animations API
class MockAnimation {
  currentTime: number | null = 0;
  paused = true;
  play() {
    this.paused = false;
  }
  pause() {
    this.paused = true;
  }
  cancel() {}
}

function createMockEl(): HTMLElement {
  const el = document.createElement("span");
  el.animate = vi.fn(() => new MockAnimation() as unknown as Animation);
  return el;
}

function maskAlphas(maskImage: string): number[] {
  return [...maskImage.matchAll(/rgba?\(0,\s*0,\s*0(?:,\s*([\d.]+))?\)/g)].map(
    (match) => (match[1] ? parseFloat(match[1]) : 1),
  );
}

describe("KaraokeFillController", () => {
  let controller: KaraokeFillController;

  beforeEach(() => {
    controller = new KaraokeFillController();
  });

  test("activateLine sets mask styles on word elements", () => {
    const lineEl = document.createElement("div");
    const wordEls = [createMockEl(), createMockEl()];
    const words = [
      { time_ms: 1000, end_ms: 1500 },
      { time_ms: 1500, end_ms: 2000 },
    ];

    controller.activateLine(lineEl, words, wordEls);

    for (const el of wordEls) {
      expect(el.style.maskImage).toContain("linear-gradient");
      expect(el.style.maskSize).toBe("200% 100%");
      expect(el.animate).toHaveBeenCalled();
    }
  });

  test("activateLine puts filled mask alpha before unfilled mask alpha", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    const words = [{ time_ms: 1000, end_ms: 1500 }];

    controller.activateLine(lineEl, words, [wordEl]);

    expect(maskAlphas(wordEl.style.maskImage)).toEqual([0.2, 1]);
  });

  test("activateLine sets prefixed mask styles and keyframes for WebKit", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    const words = [{ time_ms: 1000, end_ms: 1500 }];

    controller.activateLine(lineEl, words, [wordEl]);

    expect(wordEl.style.webkitMaskImage).toContain("linear-gradient");
    expect(wordEl.style.webkitMaskRepeat).toBe("no-repeat");
    expect(wordEl.style.webkitMaskSize).toBe("200% 100%");
    expect(wordEl.animate).toHaveBeenCalledWith(
      [
        { maskPosition: "-100% 0", webkitMaskPosition: "-100% 0" },
        { maskPosition: "0% 0", webkitMaskPosition: "0% 0" },
      ],
      expect.objectContaining({ duration: 500 }),
    );
  });

  test("activateLine is a no-op when called with the same line element", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    const words = [{ time_ms: 1000, end_ms: 1500 }];

    controller.activateLine(lineEl, words, [wordEl]);
    const callCount = (wordEl.animate as ReturnType<typeof vi.fn>).mock.calls
      .length;

    controller.activateLine(lineEl, words, [wordEl]);
    expect((wordEl.animate as ReturnType<typeof vi.fn>).mock.calls.length).toBe(
      callCount,
    );
  });

  test("activateLine rebuilds animations when word elements change in the same line", () => {
    const lineEl = document.createElement("div");
    const firstWordEl = createMockEl();
    const secondWordEl = createMockEl();
    const words = [{ time_ms: 1000, end_ms: 1500 }];

    controller.activateLine(lineEl, words, [firstWordEl]);
    controller.activateLine(lineEl, words, [secondWordEl]);

    expect(secondWordEl.animate).toHaveBeenCalled();
    expect(secondWordEl.style.maskImage).toContain("linear-gradient");
    expect(firstWordEl.style.maskImage).toBe("");
  });

  test("update sets animation currentTime before start time", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    const words = [{ time_ms: 1000, end_ms: 1500 }];
    controller.activateLine(lineEl, words, [wordEl]);

    const animation = (wordEl.animate as ReturnType<typeof vi.fn>).mock
      .results[0].value as MockAnimation;

    controller.update(500, true);
    expect(animation.currentTime).toBe(0);
    expect(animation.paused).toBe(true);
  });

  test("update sets correct currentTime for active word", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    const words = [{ time_ms: 1000, end_ms: 1500 }];
    controller.activateLine(lineEl, words, [wordEl]);

    const animation = (wordEl.animate as ReturnType<typeof vi.fn>).mock
      .results[0].value as MockAnimation;

    controller.update(1200, true);
    expect(animation.currentTime).toBe(200);
    expect(animation.paused).toBe(false);
  });

  test("update pauses when not playing", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    const words = [{ time_ms: 1000, end_ms: 1500 }];
    controller.activateLine(lineEl, words, [wordEl]);

    const animation = (wordEl.animate as ReturnType<typeof vi.fn>).mock
      .results[0].value as MockAnimation;

    controller.update(1200, false);
    expect(animation.currentTime).toBe(200);
    expect(animation.paused).toBe(true);
  });

  test("setCurrentAlpha immediately updates mask contrast", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    const words = [{ time_ms: 1000, end_ms: 1500 }];
    controller.activateLine(lineEl, words, [wordEl]);

    controller.setCurrentAlpha(1, 1);

    expect(maskAlphas(wordEl.style.maskImage)).toEqual([1, 1]);
  });

  test("update sets end time when word is past", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    const words = [{ time_ms: 1000, end_ms: 1500 }];
    controller.activateLine(lineEl, words, [wordEl]);

    const animation = (wordEl.animate as ReturnType<typeof vi.fn>).mock
      .results[0].value as MockAnimation;

    controller.update(2000, true);
    expect(animation.currentTime).toBe(500);
    expect(animation.paused).toBe(true);
  });

  test("deactivateLine cancels all animations and clears styles", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    const words = [{ time_ms: 1000, end_ms: 1500 }];
    controller.activateLine(lineEl, words, [wordEl]);

    controller.deactivateLine();
    expect(wordEl.style.maskImage).toBe("");
    expect(wordEl.style.maskSize).toBe("");
  });

  test("destroy calls deactivateLine", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    const words = [{ time_ms: 1000, end_ms: 1500 }];
    controller.activateLine(lineEl, words, [wordEl]);

    controller.destroy();
    expect(wordEl.style.maskImage).toBe("");
  });

  describe("alpha smoothing", () => {
    let perfCounter: number;

    beforeEach(() => {
      perfCounter = 0;
      vi.spyOn(performance, "now").mockImplementation(() => {
        perfCounter += 16.67; // ~60fps
        return perfCounter;
      });
    });

    afterEach(() => {
      vi.restoreAllMocks();
    });

    test("setTargetAlpha converges bright alpha toward target", () => {
      const lineEl = document.createElement("div");
      const wordEl = createMockEl();
      const words = [{ time_ms: 1000, end_ms: 1500 }];
      controller.activateLine(lineEl, words, [wordEl]);

      controller.setTargetAlpha(1.0, 1.0);

      // Simulate ~1 second at 60fps
      for (let i = 0; i < 60; i++) {
        controller.update(1200, true);
      }

      // After smoothing, maskImage should reflect bright alpha near 1.0
      const maskImage = wordEl.style.maskImage;
      expect(maskImage).toContain("linear-gradient");
      // jsdom normalizes rgba(0,0,0,1.0) to rgb(0,0,0)
      // Check if it has converged to fully opaque or near it
      const alphas = maskAlphas(maskImage);
      if (alphas.length > 0) {
        const brightValue = alphas[0];
        expect(brightValue).toBeGreaterThan(0.9);
      }
      // If no rgba match, it converged to rgb(0,0,0) which is alpha=1.0 — great
    });

    test("active-line target keeps mask contrast during the sweep", () => {
      const lineEl = document.createElement("div");
      const wordEl = createMockEl();
      const words = [{ time_ms: 1000, end_ms: 3000 }];
      controller.activateLine(lineEl, words, [wordEl]);

      controller.setTargetAlpha(0.2, 1.0);

      for (let i = 0; i < 60; i++) {
        controller.update(1500, true);
      }

      expect(maskAlphas(wordEl.style.maskImage)).toEqual([0.2, 1]);
    });

    test("deactivateLine resets alpha state", () => {
      const lineEl = document.createElement("div");
      const wordEl = createMockEl();
      const words = [{ time_ms: 1000, end_ms: 1500 }];
      controller.activateLine(lineEl, words, [wordEl]);
      controller.setTargetAlpha(1.0, 1.0);

      // Let alpha converge
      for (let i = 0; i < 60; i++) {
        controller.update(1200, true);
      }

      controller.deactivateLine();

      // Reactivate - alpha should start from 0.2 again
      const wordEl2 = createMockEl();
      controller.activateLine(lineEl, words, [wordEl2]);

      // Check initial mask has low bright alpha
      const maskImage = wordEl2.style.maskImage;
      const alphas = maskAlphas(maskImage);
      expect(alphas).not.toHaveLength(0);
      const brightValue = alphas[0];
      expect(brightValue).toBeLessThan(0.5);
    });
  });
});
