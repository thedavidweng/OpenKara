type TooltipVisibilityAction =
  | { type: "show" }
  | { type: "hide" }
  | { type: "escape" };

interface TooltipRect {
  top: number;
  left: number;
  width: number;
  height: number;
  right: number;
  bottom: number;
}

interface TooltipSize {
  width: number;
  height: number;
}

interface TooltipViewport {
  width: number;
  height: number;
}

const TOOLTIP_GAP_PX = 8;
const TOOLTIP_VIEWPORT_PADDING_PX = 8;

export function tooltipVisibilityReducer(
  _state: boolean,
  action: TooltipVisibilityAction,
): boolean {
  switch (action.type) {
    case "show":
      return true;
    case "hide":
    case "escape":
      return false;
  }
}

export function getTooltipPosition(
  anchorRect: TooltipRect,
  tooltipSize: TooltipSize,
  viewport: TooltipViewport,
): { left: number; top: number } {
  const unclampedLeft =
    anchorRect.left + anchorRect.width / 2 - tooltipSize.width / 2;
  const left = Math.max(
    Math.min(
      unclampedLeft,
      viewport.width - tooltipSize.width - TOOLTIP_VIEWPORT_PADDING_PX,
    ),
    TOOLTIP_VIEWPORT_PADDING_PX,
  );

  const topAbove = anchorRect.top - tooltipSize.height - TOOLTIP_GAP_PX;
  const top =
    topAbove >= TOOLTIP_VIEWPORT_PADDING_PX
      ? topAbove
      : Math.min(
          anchorRect.bottom + TOOLTIP_GAP_PX,
          viewport.height - tooltipSize.height - TOOLTIP_VIEWPORT_PADDING_PX,
        );

  return { left, top };
}

export interface TooltipScheduleOptions {
  delayDuration: number;
  hideGraceDuration: number;
  skipDelay: boolean;
}

export interface TooltipScheduleController {
  scheduleShow: (onShow: () => void) => void;
  scheduleHide: (onHide: () => void) => void;
  cancelAll: () => void;
}

export function createTooltipScheduleController(
  options: TooltipScheduleOptions,
): TooltipScheduleController {
  let showTimer: ReturnType<typeof setTimeout> | null = null;
  let hideTimer: ReturnType<typeof setTimeout> | null = null;

  const clearShowTimer = () => {
    if (showTimer) {
      clearTimeout(showTimer);
      showTimer = null;
    }
  };

  const clearHideTimer = () => {
    if (hideTimer) {
      clearTimeout(hideTimer);
      hideTimer = null;
    }
  };

  return {
    scheduleShow(onShow) {
      clearShowTimer();
      clearHideTimer();

      if (options.skipDelay || options.delayDuration <= 0) {
        onShow();
        return;
      }

      showTimer = setTimeout(() => {
        showTimer = null;
        onShow();
      }, options.delayDuration);
    },
    scheduleHide(onHide) {
      clearShowTimer();
      clearHideTimer();

      if (options.hideGraceDuration <= 0) {
        onHide();
        return;
      }

      hideTimer = setTimeout(() => {
        hideTimer = null;
        onHide();
      }, options.hideGraceDuration);
    },
    cancelAll() {
      clearShowTimer();
      clearHideTimer();
    },
  };
}
