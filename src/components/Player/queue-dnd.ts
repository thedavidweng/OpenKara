import type { Transform } from "@dnd-kit/utilities";

export type DropIndicatorPosition = "above" | "below";
type DropAnnouncementPosition = "before" | "after";

export function getDropIndicatorPosition(
  activeIndex: number | null,
  overIndex: number | null,
): DropIndicatorPosition | null {
  if (
    activeIndex === null ||
    overIndex === null ||
    activeIndex < 0 ||
    overIndex < 0 ||
    activeIndex === overIndex
  ) {
    return null;
  }

  return activeIndex > overIndex ? "above" : "below";
}

export function getDropAnnouncementPosition(
  activeIndex: number | null,
  overIndex: number | null,
): DropAnnouncementPosition | null {
  const indicator = getDropIndicatorPosition(activeIndex, overIndex);

  if (!indicator) {
    return null;
  }

  return indicator === "above" ? "before" : "after";
}

export function getVerticalTransform(
  transform: Transform | null,
): Transform | null {
  if (!transform) {
    return null;
  }

  return {
    ...transform,
    x: 0,
  };
}

/** Visual chrome for a queue row based on drag/overlay/drop state. */
export function getQueueItemStateClassName(options: {
  isOverlay?: boolean;
  isDraggingSource?: boolean;
  dropIndicator?: DropIndicatorPosition | null;
}): string {
  const {
    isOverlay = false,
    isDraggingSource = false,
    dropIndicator = null,
  } = options;

  if (isOverlay) {
    return "motion-safe:scale-[1.01] bg-[color-mix(in_srgb,var(--color-hover)_86%,transparent)] shadow-[0_20px_42px_rgba(0,0,0,0.34)] ring-1 ring-[color-mix(in_srgb,var(--color-accent)_65%,white)]";
  }
  if (isDraggingSource) {
    return "bg-[color-mix(in_srgb,var(--color-hover)_80%,transparent)] opacity-25";
  }
  if (dropIndicator) {
    return "bg-[color-mix(in_srgb,var(--color-hover)_80%,transparent)] shadow-[inset_0_0_0_1px_var(--color-border)]";
  }
  return "hover:bg-[color-mix(in_srgb,var(--color-hover)_76%,transparent)]";
}
