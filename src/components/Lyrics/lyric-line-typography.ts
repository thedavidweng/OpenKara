import type { LyricLine } from "@/types/ipc";
import type { LyricsPresentation } from "./lyrics-panel-model";

type FontStep = -2 | -1 | 0 | 1 | 2;

const STANDARD_SEEKABLE_HOVER_CLASS =
  "relative transition-[color,transform,text-shadow] duration-300 ease-out group-hover/line:text-[var(--color-lyrics-active)] group-hover/line:-translate-y-px group-hover/line:[text-shadow:var(--shadow-lyrics-hover)]";

const AUDIENCE_SEEKABLE_HOVER_CLASS =
  "relative transition-[transform,text-shadow] duration-300 ease-out group-hover/line:-translate-y-px";

const STANDARD_TEXT_SIZE_CLASSES = {
  [-2]: "text-lg font-semibold tracking-tight md:text-xl",
  [-1]: "text-xl font-semibold tracking-tight md:text-2xl",
  [0]: "text-2xl font-semibold tracking-tight md:text-3xl",
  [1]: "text-3xl font-semibold tracking-tight md:text-4xl xl:text-5xl",
  [2]: "text-4xl font-semibold tracking-tight md:text-5xl xl:text-6xl",
} as const;

const AUDIENCE_TEXT_SIZE_CLASSES = {
  [-2]: "text-2xl font-bold tracking-tight md:text-4xl xl:text-5xl",
  [-1]: "text-3xl font-bold tracking-tight md:text-5xl xl:text-6xl",
  [0]: "text-4xl font-bold tracking-tight md:text-6xl xl:text-7xl",
  [1]: "text-5xl font-bold tracking-tight md:text-7xl xl:text-8xl",
  [2]: "text-6xl font-bold tracking-tight md:text-8xl xl:text-8xl",
} as const;

const STANDARD_SECONDARY_TEXT_SIZE_CLASSES = {
  [-2]: "text-xs",
  [-1]: "text-xs md:text-sm",
  [0]: "text-sm md:text-base",
  [1]: "text-base md:text-lg xl:text-2xl",
  [2]: "text-lg md:text-2xl xl:text-3xl",
} as const;

const LEFT_ROMAN_TEXT_SIZE_CLASSES = {
  [-2]: "text-base font-medium tracking-tight md:text-lg",
  [-1]: "text-lg font-medium tracking-tight md:text-xl",
  [0]: "text-xl font-medium tracking-tight md:text-2xl",
  [1]: "text-2xl font-medium tracking-tight md:text-3xl xl:text-4xl",
  [2]: "text-3xl font-medium tracking-tight md:text-4xl xl:text-5xl",
} as const;

const CENTERED_LINE_FONT_STEP_SCALE = {
  [-2]: 0.76,
  [-1]: 0.88,
  [0]: 1,
  [1]: 1.14,
  [2]: 1.28,
} as const;

export const CENTERED_LINE_FONT_SIZE_BASE =
  "clamp(2.15rem, 1.7vw + 1.8vh, 3rem)";
export const CENTERED_ROMAN_FONT_SIZE = "max(0.5em, 10px)";
export const CENTERED_BG_FONT_SIZE = "max(0.7em, 10px)";
export const CENTERED_LINE_LINE_HEIGHT = 1.22;
export const CENTERED_LINE_LETTER_SPACING = "-0.02em";
export const CENTERED_LINE_PADDING = "0.42em 0.12em";

function clampStep(lyricsFontStep: number): FontStep {
  return Math.max(-2, Math.min(2, lyricsFontStep)) as FontStep;
}

export function getLyricsTextSizeClass(
  presentation: LyricsPresentation,
  lyricsFontStep: number,
): string {
  const step = clampStep(lyricsFontStep);
  return presentation === "audience"
    ? AUDIENCE_TEXT_SIZE_CLASSES[step]
    : STANDARD_TEXT_SIZE_CLASSES[step];
}

export function getLyricsSecondaryTextSizeClass(
  lyricsFontStep: number,
): string {
  return STANDARD_SECONDARY_TEXT_SIZE_CLASSES[clampStep(lyricsFontStep)];
}

export function getLyricsRomanTextSizeClass(lyricsFontStep: number): string {
  return LEFT_ROMAN_TEXT_SIZE_CLASSES[clampStep(lyricsFontStep)];
}

export function getCenteredLineFontSize(lyricsFontStep: number): string {
  const scale = CENTERED_LINE_FONT_STEP_SCALE[clampStep(lyricsFontStep)];
  if (scale === 1) {
    return CENTERED_LINE_FONT_SIZE_BASE;
  }
  return `calc(${CENTERED_LINE_FONT_SIZE_BASE} * ${scale})`;
}

export function evaluateCenteredLineFontSizePx(
  lyricsFontStep: number,
  viewportWidthPx: number,
  viewportHeightPx: number,
  rootFontSizePx = 16,
): number {
  const scale = CENTERED_LINE_FONT_STEP_SCALE[clampStep(lyricsFontStep)];
  const minPx = 2.15 * rootFontSizePx;
  const maxPx = 3 * rootFontSizePx;
  const preferredPx = 0.017 * viewportWidthPx + 0.018 * viewportHeightPx;
  return Math.min(maxPx, Math.max(minPx, preferredPx)) * scale;
}

export function getSeekableHoverClass(
  presentation: LyricsPresentation,
  isSeekable: boolean,
): string {
  if (!isSeekable) return "";
  return presentation === "audience"
    ? AUDIENCE_SEEKABLE_HOVER_CLASS
    : STANDARD_SEEKABLE_HOVER_CLASS;
}

const CJK_OR_KANA = /[一-鿿぀-ゟ゠-ヿ가-힯]/;

export function displayWordText(text: string): string {
  return text.replace(/^\s+|\s+$/g, "");
}

export function wordTokenGap(
  current: string,
  next: string | undefined,
): string {
  if (next === undefined) {
    return "";
  }
  const left = displayWordText(current);
  const right = displayWordText(next);
  if (!left || !right) {
    return "";
  }
  if (CJK_OR_KANA.test(left) && CJK_OR_KANA.test(right)) {
    return "";
  }
  return " ";
}

/** A word long enough, and short enough, to carry the karaoke glow on its own. */
export function shouldEmphasizeWord(word: {
  text: string;
  time_ms: number;
  end_ms: number;
}): boolean {
  const duration = word.end_ms - word.time_ms;
  if (duration < 1000) return false;
  const trimmed = word.text.trim();
  if (/[一-鿿぀-ゟ゠-ヿ]/.test(trimmed)) return true;
  return trimmed.length >= 2 && trimmed.length <= 7;
}

export function isLastWord(index: number, total: number): boolean {
  return index === total - 1;
}

export function hasBackgroundWords(line: LyricLine): boolean {
  return line.bg_words !== null && line.bg_words.length > 0;
}
