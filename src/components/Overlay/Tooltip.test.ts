import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import {
  createTooltipScheduleController,
  getTooltipPosition,
  tooltipVisibilityReducer,
} from "./Tooltip.utils";

describe("tooltipVisibilityReducer", () => {
  test("opens on show", () => {
    expect(tooltipVisibilityReducer(false, { type: "show" })).toBe(true);
  });

  test("closes on hide and escape", () => {
    expect(tooltipVisibilityReducer(true, { type: "hide" })).toBe(false);
    expect(tooltipVisibilityReducer(true, { type: "escape" })).toBe(false);
  });
});

describe("getTooltipPosition", () => {
  test("centers above the trigger when space is available", () => {
    expect(
      getTooltipPosition(
        {
          top: 80,
          left: 100,
          width: 32,
          height: 32,
          bottom: 112,
          right: 132,
        },
        { width: 120, height: 40 },
        { width: 400, height: 300 },
      ),
    ).toEqual({ left: 56, top: 32 });
  });

  test("falls back below the trigger and clamps to the viewport", () => {
    expect(
      getTooltipPosition(
        {
          top: 8,
          left: 4,
          width: 32,
          height: 32,
          bottom: 40,
          right: 36,
        },
        { width: 180, height: 40 },
        { width: 200, height: 120 },
      ),
    ).toEqual({ left: 8, top: 48 });
  });
});

describe("createTooltipScheduleController", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
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
