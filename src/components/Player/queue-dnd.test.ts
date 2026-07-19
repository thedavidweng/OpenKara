import { describe, expect, test } from "vitest";
import {
  getDropAnnouncementPosition,
  getDropIndicatorPosition,
  getQueueItemStateClassName,
  getVerticalTransform,
} from "./queue-dnd";

describe("getDropIndicatorPosition", () => {
  test("shows the indicator above the hovered item when dragging upward", () => {
    expect(getDropIndicatorPosition(4, 1)).toBe("above");
  });

  test("shows the indicator below the hovered item when dragging downward", () => {
    expect(getDropIndicatorPosition(1, 4)).toBe("below");
  });

  test("maps indicator placement to announcement wording", () => {
    expect(getDropAnnouncementPosition(4, 1)).toBe("before");
    expect(getDropAnnouncementPosition(1, 4)).toBe("after");
  });

  test("removes horizontal motion from sortable transforms in a vertical list", () => {
    expect(
      getVerticalTransform({
        x: 28,
        y: -96,
        scaleX: 1,
        scaleY: 1,
      }),
    ).toEqual({
      x: 0,
      y: -96,
      scaleX: 1,
      scaleY: 1,
    });
  });

  test("hides the indicator when there is no valid target change", () => {
    expect(getDropIndicatorPosition(null, 2)).toBeNull();
    expect(getDropIndicatorPosition(2, null)).toBeNull();
    expect(getDropIndicatorPosition(2, 2)).toBeNull();
    expect(getDropAnnouncementPosition(2, 2)).toBeNull();
    expect(getVerticalTransform(null)).toBeNull();
  });
});

describe("getQueueItemStateClassName", () => {
  test("uses theme border token for drop-indicator chrome", () => {
    expect(getQueueItemStateClassName({ dropIndicator: "above" })).toContain(
      "var(--color-border)",
    );
    expect(getQueueItemStateClassName({ dropIndicator: "below" })).toContain(
      "shadow-[inset_0_0_0_1px_var(--color-border)]",
    );
  });

  test("covers overlay, dragging, and idle hover states", () => {
    expect(getQueueItemStateClassName({ isOverlay: true })).toContain(
      "motion-safe:scale-[1.01]",
    );
    expect(getQueueItemStateClassName({ isDraggingSource: true })).toContain(
      "opacity-25",
    );
    expect(getQueueItemStateClassName({})).toContain("hover:bg-");
  });
});
