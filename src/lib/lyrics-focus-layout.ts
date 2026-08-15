export const FOCUS_LINE_GAP_PX = 16;
export const FOCUS_HEAD_PAD_RATIO = 0.38;

export function focusHeadPadPx(viewportHeight: number): number {
  return Math.max(48, viewportHeight * FOCUS_HEAD_PAD_RATIO);
}

export function layoutFocusLineTops(
  heights: number[],
  gapPx: number,
  headPadPx: number,
): { tops: number[]; stageHeight: number } {
  const tops: number[] = [];
  let y = headPadPx;
  for (let index = 0; index < heights.length; index += 1) {
    tops.push(y);
    y += Math.max(1, heights[index]);
    if (index < heights.length - 1) {
      y += gapPx;
    }
  }
  return { tops, stageHeight: y + headPadPx };
}

export function canUseMeasuredFocusLayout(heights: number[]): boolean {
  return heights.some((height) => height > 1);
}
