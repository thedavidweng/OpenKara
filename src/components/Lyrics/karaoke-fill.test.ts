// @vitest-environment jsdom
import { describe, expect, test, beforeEach } from "vitest";
import {
  applyWordMask,
  fadeGradientSpec,
  karaokeFillProgress,
  maskOffsetPx,
  KaraokeFillController,
} from "./karaoke-fill";

function createMockEl(width = 80, height = 40): HTMLElement {
  const el = document.createElement("span");
  Object.defineProperty(el, "clientWidth", {
    configurable: true,
    value: width,
  });
  Object.defineProperty(el, "clientHeight", {
    configurable: true,
    value: height,
  });
  return el;
}

describe("fadeGradientSpec", () => {
  test("puts the bright stop left of the dark stop for a left-to-right wipe", () => {
    const spec = fadeGradientSpec(0.5);
    expect(spec.image.startsWith("linear-gradient(to right")).toBe(true);
    expect(spec.image).toContain("var(--bright-mask-alpha");
    expect(spec.image).toContain("var(--dark-mask-alpha");
    expect(spec.sizePercent).toBeGreaterThan(200);

    const brightIndex = spec.image.indexOf("--bright-mask-alpha");
    const darkIndex = spec.image.indexOf("--dark-mask-alpha");
    expect(brightIndex).toBeLessThan(darkIndex);
  });
});

describe("maskOffsetPx", () => {
  test("starts left of the word and ends at zero", () => {
    expect(maskOffsetPx(0, 80, 20)).toBe(-100);
    expect(maskOffsetPx(1, 80, 20)).toBe(0);
    expect(maskOffsetPx(0.5, 80, 20)).toBe(-50);
  });
});

describe("karaokeFillProgress", () => {
  test("stays closed before the word and open after it", () => {
    expect(karaokeFillProgress(500, 1000, 1500)).toBe(0);
    expect(karaokeFillProgress(1500, 1000, 1500)).toBe(1);
    expect(karaokeFillProgress(1250, 1000, 1500)).toBeCloseTo(0.5);
  });
});

describe("KaraokeFillController", () => {
  let controller: KaraokeFillController;

  beforeEach(() => {
    controller = new KaraokeFillController();
  });

  test("activateLine applies a to-right fade mask and starts unsung", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    controller.activateLine(
      lineEl,
      [{ time_ms: 1000, end_ms: 1500 }],
      [wordEl],
    );

    expect(wordEl.style.maskSize || wordEl.style.webkitMaskSize).toContain("%");
    expect(wordEl.style.maskPosition || wordEl.style.webkitMaskPosition).toBe(
      "-100px 0px",
    );
  });

  test("update wipes the mask from left to right in pixels", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    controller.activateLine(
      lineEl,
      [{ time_ms: 1000, end_ms: 1500 }],
      [wordEl],
    );

    controller.update(1250, true);
    expect(wordEl.style.maskPosition || wordEl.style.webkitMaskPosition).toBe(
      "-50px 0px",
    );

    controller.update(2000, true);
    expect(wordEl.style.maskPosition || wordEl.style.webkitMaskPosition).toBe(
      "0px 0px",
    );
  });

  test("activateLine is a no-op when called with the same binding", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    const words = [{ time_ms: 1000, end_ms: 1500 }];
    controller.activateLine(lineEl, words, [wordEl]);
    wordEl.style.maskPosition = "-12px 0px";
    controller.activateLine(lineEl, words, [wordEl]);
    expect(wordEl.style.maskPosition).toBe("-12px 0px");
  });

  test("deactivateLine keeps the mask so inactive dual-alpha can stay applied", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl();
    controller.activateLine(
      lineEl,
      [{ time_ms: 1000, end_ms: 1500 }],
      [wordEl],
    );
    expect(wordEl.style.maskSize || wordEl.style.webkitMaskSize).toContain("%");
    controller.deactivateLine();
    expect(wordEl.style.maskSize || wordEl.style.webkitMaskSize).toContain("%");
  });

  test("update remasures when the word box changes size", () => {
    const lineEl = document.createElement("div");
    const wordEl = createMockEl(10, 10);
    controller.activateLine(
      lineEl,
      [{ time_ms: 1000, end_ms: 1500 }],
      [wordEl],
    );
    Object.defineProperty(wordEl, "clientWidth", { value: 80 });
    Object.defineProperty(wordEl, "clientHeight", { value: 40 });
    controller.update(1250, true);
    expect(wordEl.style.maskPosition || wordEl.style.webkitMaskPosition).toBe(
      "-50px 0px",
    );
  });

  test("applyWordMask sizes the gradient from word height", () => {
    const el = createMockEl(100, 40);
    const measured = applyWordMask(el);
    expect(measured.width).toBe(100);
    expect(measured.fade).toBe(20);
    expect(el.style.maskRepeat).toBe("no-repeat");
  });
});
