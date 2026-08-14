export const INACTIVE_MASK_ALPHA = 0.2;
export const ACTIVE_BRIGHT_ALPHA = 1;
export const ACTIVE_DARK_ALPHA = 0.18;
export const WORD_FADE_HEIGHT_RATIO = 0.5;

const BRIGHT = "rgba(0,0,0,var(--bright-mask-alpha, 1))";
const DARK = "rgba(0,0,0,var(--dark-mask-alpha, 1))";

type MaskStyle = CSSStyleDeclaration & {
  webkitMaskImage: string;
  webkitMaskRepeat: string;
  webkitMaskOrigin: string;
  webkitMaskSize: string;
  webkitMaskPosition: string;
};

export function fadeGradientSpec(fadeToWordRatio: number): {
  image: string;
  sizePercent: number;
} {
  const width = Math.max(0.0001, fadeToWordRatio);
  const totalAspect = 2 + width;
  const widthInTotal = width / totalAspect;
  const leftPos = (1 - widthInTotal) / 2;
  return {
    image: `linear-gradient(to right,${BRIGHT} ${leftPos * 100}%,${DARK} ${(leftPos + widthInTotal) * 100}%)`,
    sizePercent: totalAspect * 100,
  };
}

export function maskOffsetPx(
  progress: number,
  wordWidth: number,
  fadeWidth: number,
): number {
  const travel = wordWidth + fadeWidth;
  const p = Math.min(1, Math.max(0, progress));
  return -travel + p * travel;
}

export function karaokeFillProgress(
  currentMs: number,
  startMs: number,
  endMs: number,
): number {
  if (currentMs <= startMs) {
    return 0;
  }
  if (currentMs >= endMs) {
    return 1;
  }
  return (currentMs - startMs) / Math.max(1, endMs - startMs);
}

function measureWord(element: HTMLElement): {
  width: number;
  fade: number;
} {
  const width = Math.max(1, element.clientWidth);
  const height = Math.max(1, element.clientHeight);
  return { width, fade: height * WORD_FADE_HEIGHT_RATIO };
}

export function applyWordMask(element: HTMLElement): {
  width: number;
  fade: number;
} {
  const measured = measureWord(element);
  const spec = fadeGradientSpec(measured.fade / measured.width);
  const style = element.style as MaskStyle;
  const size = `${spec.sizePercent}% 100%`;
  style.maskImage = spec.image;
  style.maskRepeat = "no-repeat";
  style.maskOrigin = "left";
  style.maskSize = size;
  style.webkitMaskImage = spec.image;
  style.webkitMaskRepeat = "no-repeat";
  style.webkitMaskOrigin = "left";
  style.webkitMaskSize = size;
  return measured;
}

export function setWordMaskProgress(
  element: HTMLElement,
  progress: number,
  wordWidth: number,
  fadeWidth: number,
) {
  const position = `${maskOffsetPx(progress, wordWidth, fadeWidth)}px 0px`;
  const style = element.style as MaskStyle;
  style.maskPosition = position;
  style.webkitMaskPosition = position;
}

export function clearWordMask(element: HTMLElement) {
  const style = element.style as MaskStyle;
  style.maskImage = "";
  style.maskRepeat = "";
  style.maskOrigin = "";
  style.maskSize = "";
  style.maskPosition = "";
  style.webkitMaskImage = "";
  style.webkitMaskRepeat = "";
  style.webkitMaskOrigin = "";
  style.webkitMaskSize = "";
  style.webkitMaskPosition = "";
}

interface WordFill {
  element: HTMLElement;
  startTime: number;
  endTime: number;
  width: number;
  fade: number;
}

export class KaraokeFillController {
  private wordFills: WordFill[] = [];
  private activeLineEl: HTMLElement | null = null;
  private activeWordEls: HTMLElement[] = [];
  private activeWordTimings: Array<{ time_ms: number; end_ms: number }> = [];
  private wordResizeObserver: ResizeObserver | null = null;

  activateLine(
    lineEl: HTMLElement,
    words: Array<{ time_ms: number; end_ms: number }>,
    wordEls: HTMLElement[],
  ) {
    if (this.activeLineEl === lineEl && this.hasSameBinding(words, wordEls)) {
      return;
    }
    this.deactivateLine();
    this.activeLineEl = lineEl;

    for (let i = 0; i < words.length && i < wordEls.length; i++) {
      const word = words[i];
      const el = wordEls[i];
      const measured = applyWordMask(el);
      setWordMaskProgress(el, 0, measured.width, measured.fade);
      this.wordFills.push({
        element: el,
        startTime: word.time_ms,
        endTime: word.end_ms,
        width: measured.width,
        fade: measured.fade,
      });
    }
    this.activeWordEls = wordEls.slice(0, this.wordFills.length);
    this.activeWordTimings = words.slice(0, this.wordFills.length);
    this.observeWordSizes();
  }

  update(currentMs: number, _isPlaying: boolean) {
    if (!this.wordResizeObserver) {
      this.refreshMeasuredWords();
    }
    for (const word of this.wordFills) {
      setWordMaskProgress(
        word.element,
        karaokeFillProgress(currentMs, word.startTime, word.endTime),
        word.width,
        word.fade,
      );
    }
  }

  deactivateLine() {
    this.disconnectWordObserver();
    this.wordFills = [];
    this.activeLineEl = null;
    this.activeWordEls = [];
    this.activeWordTimings = [];
  }

  private observeWordSizes() {
    this.disconnectWordObserver();
    if (typeof ResizeObserver === "undefined") {
      return;
    }
    this.wordResizeObserver = new ResizeObserver(() => {
      this.refreshMeasuredWords();
    });
    for (const word of this.wordFills) {
      this.wordResizeObserver.observe(word.element);
    }
  }

  private disconnectWordObserver() {
    this.wordResizeObserver?.disconnect();
    this.wordResizeObserver = null;
  }

  private refreshMeasuredWords() {
    for (const word of this.wordFills) {
      const measured = measureWord(word.element);
      if (measured.width !== word.width || measured.fade !== word.fade) {
        applyWordMask(word.element);
        word.width = measured.width;
        word.fade = measured.fade;
      }
    }
  }

  destroy() {
    this.deactivateLine();
  }

  private hasSameBinding(
    words: Array<{ time_ms: number; end_ms: number }>,
    wordEls: HTMLElement[],
  ) {
    const bindingLength = Math.min(words.length, wordEls.length);
    if (bindingLength !== this.activeWordEls.length) {
      return false;
    }

    for (let index = 0; index < bindingLength; index += 1) {
      const activeTiming = this.activeWordTimings[index];
      const nextTiming = words[index];
      if (
        this.activeWordEls[index] !== wordEls[index] ||
        activeTiming.time_ms !== nextTiming.time_ms ||
        activeTiming.end_ms !== nextTiming.end_ms
      ) {
        return false;
      }
    }

    return true;
  }
}
