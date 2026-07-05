import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import {
  createTooltipScheduleController,
  tooltipVisibilityReducer,
  getTooltipPosition,
} from "./Tooltip.utils";

describe("tooltipVisibilityReducer", () => {
  test("show sets visibility to true", () => {
    expect(tooltipVisibilityReducer(false, { type: "show" })).toBe(true);
  });

  test("show keeps true if already true", () => {
    expect(tooltipVisibilityReducer(true, { type: "show" })).toBe(true);
  });

  test("hide sets visibility to false", () => {
    expect(tooltipVisibilityReducer(true, { type: "hide" })).toBe(false);
  });

  test("hide keeps false if already false", () => {
    expect(tooltipVisibilityReducer(false, { type: "hide" })).toBe(false);
  });

  test("escape sets visibility to false", () => {
    expect(tooltipVisibilityReducer(true, { type: "escape" })).toBe(false);
  });

  test("escape from already-hidden state stays false", () => {
    expect(tooltipVisibilityReducer(false, { type: "escape" })).toBe(false);
  });
});

describe("getTooltipPosition", () => {
  const VIEWPORT_PADDING = 8;

  test("centers tooltip horizontally above the anchor when there is room", () => {
    const anchorRect = {
      left: 100,
      top: 200,
      width: 80,
      height: 30,
      right: 180,
      bottom: 230,
    };
    const tooltipSize = { width: 120, height: 40 };
    const viewport = { width: 800, height: 600 };

    const result = getTooltipPosition(anchorRect, tooltipSize, viewport);

    expect(result.left).toBe(80);
    expect(result.top).toBe(152);
  });

  test("falls back to below anchor when not enough space above", () => {
    const anchorRect = {
      left: 100,
      top: 20,
      width: 80,
      height: 30,
      right: 180,
      bottom: 50,
    };
    const tooltipSize = { width: 120, height: 40 };
    const viewport = { width: 800, height: 600 };

    const result = getTooltipPosition(anchorRect, tooltipSize, viewport);

    expect(result.top).toBe(58);
  });

  test("clamps tooltip to the left viewport edge", () => {
    const anchorRect = {
      left: 5,
      top: 200,
      width: 40,
      height: 30,
      right: 45,
      bottom: 230,
    };
    const tooltipSize = { width: 200, height: 40 };
    const viewport = { width: 800, height: 600 };

    const result = getTooltipPosition(anchorRect, tooltipSize, viewport);

    expect(result.left).toBe(VIEWPORT_PADDING);
  });

  test("clamps tooltip to the right viewport edge", () => {
    const anchorRect = {
      left: 700,
      top: 200,
      width: 80,
      height: 30,
      right: 780,
      bottom: 230,
    };
    const tooltipSize = { width: 200, height: 40 };
    const viewport = { width: 800, height: 600 };

    const result = getTooltipPosition(anchorRect, tooltipSize, viewport);

    expect(result.left).toBe(592);
  });

  test("clamps fallback-below position to viewport bottom", () => {
    const anchorRect = {
      left: 100,
      top: 10,
      width: 80,
      height: 30,
      right: 180,
      bottom: 40,
    };
    const tooltipSize = { width: 120, height: 40 };
    const viewport = { width: 800, height: 100 };

    const result = getTooltipPosition(anchorRect, tooltipSize, viewport);

    expect(result.top).toBe(48);
  });

  test("when tooltip is wider than viewport, left clamps to VIEWPORT_PADDING", () => {
    const anchorRect = {
      left: 100,
      top: 200,
      width: 80,
      height: 30,
      right: 180,
      bottom: 230,
    };
    const tooltipSize = { width: 1000, height: 40 };
    const viewport = { width: 800, height: 600 };

    const result = getTooltipPosition(anchorRect, tooltipSize, viewport);

    expect(result.left).toBe(VIEWPORT_PADDING);
  });
});

describe("createTooltipScheduleController", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  test("cancelAll prevents pending callbacks", () => {
    const onShow = vi.fn();
    const onHide = vi.fn();
    const controller = createTooltipScheduleController({
      delayDuration: 600,
      hideGraceDuration: 120,
      skipDelay: false,
    });

    controller.scheduleShow(onShow);
    controller.cancelAll();
    vi.advanceTimersByTime(1000);
    expect(onShow).not.toHaveBeenCalled();

    controller.scheduleHide(onHide);
    controller.cancelAll();
    vi.advanceTimersByTime(1000);
    expect(onHide).not.toHaveBeenCalled();
  });

  test("hides immediately when hideGraceDuration is zero", () => {
    const onHide = vi.fn();
    const controller = createTooltipScheduleController({
      delayDuration: 600,
      hideGraceDuration: 0,
      skipDelay: false,
    });

    controller.scheduleHide(onHide);
    expect(onHide).toHaveBeenCalledTimes(1);
  });

  test("waits for delayDuration before showing", () => {
    const onShow = vi.fn();
    const controller = createTooltipScheduleController({
      delayDuration: 600,
      hideGraceDuration: 120,
      skipDelay: false,
    });

    controller.scheduleShow(onShow);
    expect(onShow).not.toHaveBeenCalled();

    vi.advanceTimersByTime(599);
    expect(onShow).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1);
    expect(onShow).toHaveBeenCalledTimes(1);
  });

  test("shows immediately when skipDelay is active", () => {
    const onShow = vi.fn();
    const controller = createTooltipScheduleController({
      delayDuration: 600,
      hideGraceDuration: 120,
      skipDelay: true,
    });

    controller.scheduleShow(onShow);
    expect(onShow).toHaveBeenCalledTimes(1);
  });

  test("waits for hideGraceDuration before hiding", () => {
    const onHide = vi.fn();
    const controller = createTooltipScheduleController({
      delayDuration: 600,
      hideGraceDuration: 120,
      skipDelay: false,
    });

    controller.scheduleHide(onHide);
    expect(onHide).not.toHaveBeenCalled();

    vi.advanceTimersByTime(120);
    expect(onHide).toHaveBeenCalledTimes(1);
  });

  test("cancels a pending show when hide is scheduled", () => {
    const onShow = vi.fn();
    const onHide = vi.fn();
    const controller = createTooltipScheduleController({
      delayDuration: 600,
      hideGraceDuration: 120,
      skipDelay: false,
    });

    controller.scheduleShow(onShow);
    controller.scheduleHide(onHide);

    vi.advanceTimersByTime(600);
    expect(onShow).not.toHaveBeenCalled();
    expect(onHide).toHaveBeenCalledTimes(1);
  });
});
