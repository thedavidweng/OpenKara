import { memo } from "react";
import {
  buildAudiencePresentationSpec,
  colorToCss,
} from "@/lib/audience-presentation";
import { usePlayerStore } from "@/stores/player-store";
import type { LyricLine as LyricLineType, WordToken } from "@/types/ipc";

interface LyricLineProps {
  line: LyricLineType;
  state: "active" | "past" | "future" | "plain";
  adjustedMs: number;
  presentation?: "standard" | "audience";
  lyricsFontStep: number;
  romanizedText?: string;
}

const STANDARD_TEXT_SIZE_CLASSES = {
  [-2]: "text-lg font-bold tracking-tight md:text-xl",
  [-1]: "text-xl font-bold tracking-tight md:text-2xl",
  [0]: "text-2xl font-bold tracking-tight md:text-3xl",
  [1]: "text-3xl font-bold tracking-tight md:text-4xl xl:text-5xl",
  [2]: "text-4xl font-bold tracking-tight md:text-5xl xl:text-6xl",
} as const;

const AUDIENCE_TEXT_SIZE_CLASSES = {
  [-2]: "text-2xl font-bold tracking-tight md:text-4xl xl:text-5xl",
  [-1]: "text-3xl font-bold tracking-tight md:text-5xl xl:text-6xl",
  [0]: "text-4xl font-bold tracking-tight md:text-6xl xl:text-7xl",
  [1]: "text-5xl font-bold tracking-tight md:text-7xl xl:text-8xl",
  [2]: "text-6xl font-bold tracking-tight md:text-8xl xl:text-8xl",
} as const;

function getLyricsTextSizeClass(
  presentation: "standard" | "audience",
  lyricsFontStep: number,
): string {
  const clampedStep = Math.max(-2, Math.min(2, lyricsFontStep)) as
    | -2
    | -1
    | 0
    | 1
    | 2;

  return presentation === "audience"
    ? AUDIENCE_TEXT_SIZE_CLASSES[clampedStep]
    : STANDARD_TEXT_SIZE_CLASSES[clampedStep];
}

function getActiveWordIndex(words: WordToken[], adjustedMs: number): number {
  let activeIndex = -1;

  for (let index = 0; index < words.length; index += 1) {
    if (words[index].time_ms > adjustedMs) {
      break;
    }
    activeIndex = index;
  }

  return activeIndex;
}

function areLyricLinePropsEqual(
  previous: LyricLineProps,
  next: LyricLineProps,
): boolean {
  if (
    previous.line !== next.line ||
    previous.state !== next.state ||
    previous.presentation !== next.presentation ||
    previous.lyricsFontStep !== next.lyricsFontStep ||
    previous.romanizedText !== next.romanizedText
  ) {
    return false;
  }

  if (previous.state !== "active" && next.state !== "active") {
    return true;
  }

  return previous.adjustedMs === next.adjustedMs;
}

export const LyricLine = memo(function LyricLine({
  line,
  state,
  adjustedMs,
  presentation = "standard",
  lyricsFontStep,
  romanizedText,
}: LyricLineProps) {
  const seek = usePlayerStore((s) => s.seek);
  const isSeekable = state !== "plain";
  const textSizeClass = getLyricsTextSizeClass(presentation, lyricsFontStep);
  const audiencePresentationSpec =
    buildAudiencePresentationSpec(lyricsFontStep);

  const handleClick = () => {
    if (!isSeekable) return;
    seek(line.time_ms);
  };

  const hasWords = line.words !== null && line.words.length > 0;
  const activeWordIndex =
    hasWords && state === "active"
      ? getActiveWordIndex(line.words!, adjustedMs)
      : -1;
  const hoverClass = isSeekable
    ? "group-hover/line:underline decoration-2 underline-offset-4"
    : "";

  return (
    <div
      onClick={isSeekable ? handleClick : undefined}
      className={`motion-surface flex flex-col items-center gap-1.5 text-center ${
        state === "active" ? "opacity-100" : "opacity-70"
      } ${isSeekable ? "cursor-pointer group/line" : ""}`}
      style={{
        fontFamily:
          '-apple-system, BlinkMacSystemFont, "Helvetica Neue", "Noto Sans SC", "Noto Sans JP", "Noto Sans KR", system-ui, sans-serif',
        ...(presentation === "audience"
          ? {
              transform:
                state === "active"
                  ? `scale(${audiencePresentationSpec.activeScale})`
                  : undefined,
            }
          : undefined),
      }}
    >
      {hasWords ? (
        <span
          className={(presentation === "audience"
            ? `tracking-tight ${hoverClass}`
            : `${textSizeClass} ${hoverClass}`
          ).trim()}
          style={{
            fontWeight: state === "active" ? 500 : 400,
            ...(presentation === "audience"
              ? {
                  fontSize: audiencePresentationSpec.fontSizePx,
                  lineHeight: audiencePresentationSpec.lineHeightMultiple,
                }
              : undefined),
          }}
        >
          {line.words!.map((word, idx) => {
            const wordState =
              state === "plain"
                ? "active"
                : state === "active"
                  ? idx < activeWordIndex
                    ? "past"
                    : idx === activeWordIndex
                      ? "active"
                      : "future"
                  : state === "past"
                    ? "past"
                    : "future";

            const isActiveWord = wordState === "active";
            const isPastWord = wordState === "past";

            // Calculate fill progress for active word
            const wordDuration =
              idx < line.words!.length - 1
                ? line.words![idx + 1].time_ms - word.time_ms
                : 500; // default 500ms for last word
            const elapsed = Math.max(0, adjustedMs - word.time_ms);
            const progress = isActiveWord
              ? Math.min(1, elapsed / Math.max(wordDuration, 1))
              : isPastWord
                ? 1
                : 0;

            return (
              <span
                key={idx}
                className={
                  presentation === "audience"
                    ? "motion-surface"
                    : `motion-surface relative inline-block ${
                        wordState === "active"
                          ? "text-white"
                          : wordState === "past"
                            ? "text-[var(--color-text-dimmer)]"
                            : "text-[var(--color-active)]"
                      }`
                }
                style={{
                  ...(presentation === "audience"
                    ? {
                        color: colorToCss(
                          wordState === "active"
                            ? audiencePresentationSpec.activeTextColor
                            : wordState === "past"
                              ? audiencePresentationSpec.pastTextColor
                              : audiencePresentationSpec.futureTextColor,
                        ),
                        textShadow:
                          wordState === "active"
                            ? `0 0 ${audiencePresentationSpec.activeGlowBlurPx}px ${colorToCss(
                                audiencePresentationSpec.activeGlowColor,
                              )}`
                            : undefined,
                      }
                    : isActiveWord
                      ? {
                          textShadow:
                            "0 0 12px rgba(255,255,255,0.5), 0 0 4px rgba(255,255,255,0.4)",
                        }
                      : undefined),
                  ...(isActiveWord && presentation !== "audience"
                    ? {
                        WebkitMaskImage: `linear-gradient(to right, rgba(0,0,0,1) ${progress * 100}%, rgba(0,0,0,0.2) ${progress * 100}%)`,
                        WebkitMaskRepeat: "no-repeat",
                        WebkitMaskOrigin: "left",
                        maskImage: `linear-gradient(to right, rgba(0,0,0,1) ${progress * 100}%, rgba(0,0,0,0.2) ${progress * 100}%)`,
                        maskRepeat: "no-repeat",
                        maskOrigin: "left",
                      }
                    : {}),
                }}
              >
                {word.text}
                {idx < line.words!.length - 1 ? " " : ""}
              </span>
            );
          })}
        </span>
      ) : (
        <span
          className={(presentation === "audience"
            ? `motion-surface font-bold tracking-tight ${hoverClass}`
            : `motion-surface ${textSizeClass} ${
                state === "plain" || state === "active"
                  ? "text-white"
                  : state === "past"
                    ? "text-[var(--color-text-dimmer)]"
                    : "text-[var(--color-active)]"
              } ${hoverClass}`
          ).trim()}
          style={
            presentation === "audience"
              ? {
                  fontSize: audiencePresentationSpec.fontSizePx,
                  lineHeight: audiencePresentationSpec.lineHeightMultiple,
                  color: colorToCss(
                    state === "plain" || state === "active"
                      ? audiencePresentationSpec.activeTextColor
                      : state === "past"
                        ? audiencePresentationSpec.pastTextColor
                        : audiencePresentationSpec.futureTextColor,
                  ),
                  textShadow:
                    state === "active"
                      ? `0 0 ${audiencePresentationSpec.activeGlowBlurPx}px ${colorToCss(
                          audiencePresentationSpec.activeGlowColor,
                        )}`
                      : undefined,
                }
              : undefined
          }
        >
          {line.text}
        </span>
      )}
      {line.bg_words && line.bg_words.length > 0 ? (
        <span
          className={
            presentation === "audience"
              ? "motion-surface font-medium tracking-tight opacity-40"
              : `motion-surface text-sm font-medium md:text-base opacity-40 ${
                  state === "plain" || state === "active"
                    ? "text-[var(--color-text-dim)]"
                    : state === "past"
                      ? "text-[var(--color-text-dimmer)]"
                      : "text-[var(--color-text-dim)]"
                }`
          }
          style={
            presentation === "audience"
              ? {
                  fontSize: audiencePresentationSpec.fontSizePx * 0.55,
                  lineHeight: audiencePresentationSpec.lineHeightMultiple,
                  color: colorToCss(
                    state === "plain" || state === "active"
                      ? audiencePresentationSpec.activeTextColor
                      : state === "past"
                        ? audiencePresentationSpec.pastTextColor
                        : audiencePresentationSpec.futureTextColor,
                  ),
                  opacity: 0.4,
                }
              : undefined
          }
        >
          {line.bg_words.map((word, idx) => (
            <span key={idx}>
              {word.text}
              {idx < line.bg_words!.length - 1 ? " " : ""}
            </span>
          ))}
        </span>
      ) : null}
      {romanizedText ? (
        <span
          className={
            presentation === "audience"
              ? "motion-surface font-medium tracking-tight opacity-50"
              : `motion-surface text-sm font-medium md:text-base ${
                  state === "plain" || state === "active"
                    ? "text-[var(--color-text-dim)]"
                    : state === "past"
                      ? "text-[var(--color-text-dimmer)]"
                      : "text-[var(--color-text-dim)]"
                }`
          }
          style={
            presentation === "audience"
              ? {
                  fontSize: audiencePresentationSpec.fontSizePx * 0.55,
                  lineHeight: audiencePresentationSpec.lineHeightMultiple,
                  color: colorToCss(
                    state === "plain" || state === "active"
                      ? audiencePresentationSpec.activeTextColor
                      : state === "past"
                        ? audiencePresentationSpec.pastTextColor
                        : audiencePresentationSpec.futureTextColor,
                  ),
                  opacity: 0.45,
                }
              : undefined
          }
        >
          {romanizedText}
        </span>
      ) : null}
    </div>
  );
}, areLyricLinePropsEqual);
