import { memo, useRef, useEffect } from "react";
import { KaraokeFillController } from "./karaoke-fill";
import {
  buildAudiencePresentationSpec,
  colorToCss,
} from "@/lib/audience-presentation";
import { lyricsLineRuntime } from "@/lib/lyrics-line-runtime";
import { usePlayerStore } from "@/stores/player-store";
import type { LyricLine as LyricLineType } from "@/types/ipc";

interface LyricLineProps {
  lineIndex: number;
  line: LyricLineType;
  state: "active" | "past" | "future" | "plain";
  activeWordIndex?: number;
  presentation?: "standard" | "audience";
  lyricsFontStep: number;
  romanizedText?: string;
}

const SEEKABLE_HOVER_CLASS =
  "relative transition-[color,transform,text-shadow] duration-300 ease-out group-hover/line:text-white group-hover/line:-translate-y-px group-hover/line:[text-shadow:0_2px_4px_rgba(0,0,0,0.55),0_10px_36px_rgba(0,0,0,0.42),0_22px_64px_rgba(0,0,0,0.28),0_0_48px_rgba(255,255,255,0.14)]";

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

function shouldEmphasize(word: {
  text: string;
  time_ms: number;
  end_ms: number;
}): boolean {
  const duration = word.end_ms - word.time_ms;
  if (duration < 1000) return false;
  const trimmed = word.text.trim();
  // CJK characters: any length qualifies
  if (/[一-鿿぀-ゟ゠-ヿ]/.test(trimmed)) return true;
  // Non-CJK: 2-7 characters
  return trimmed.length >= 2 && trimmed.length <= 7;
}

function isLastWord(index: number, total: number): boolean {
  return index === total - 1;
}

function hasBackgroundWords(line: LyricLineType): boolean {
  return line.bg_words !== null && line.bg_words.length > 0;
}

function areLyricLinePropsEqual(
  previous: LyricLineProps,
  next: LyricLineProps,
): boolean {
  if (
    previous.lineIndex !== next.lineIndex ||
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

  return (previous.activeWordIndex ?? -1) === (next.activeWordIndex ?? -1);
}

export const LyricLine = memo(function LyricLine({
  lineIndex,
  line,
  state,
  activeWordIndex = -1,
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
    // The player store publishes the lyrics seek edge after the asynchronous
    // Tauri command installs its authoritative target snapshot.
    void seek(line.time_ms);
  };

  const words = line.words;
  const hasWords = words !== null && words.length > 0;
  const hasOnlyBackgroundWords = !hasWords && hasBackgroundWords(line);
  const shouldUseKaraokeFill =
    state === "active" && hasWords && presentation !== "audience";
  const hoverClass = isSeekable ? SEEKABLE_HOVER_CLASS : "";

  const karaokeRef = useRef<KaraokeFillController | null>(null);
  const wordElsRef = useRef<HTMLElement[]>([]);
  const wordsRef = useRef(words);
  wordsRef.current = words;

  useEffect(() => {
    if (shouldUseKaraokeFill) {
      if (!karaokeRef.current) {
        karaokeRef.current = new KaraokeFillController();
      }
      const container = wordElsRef.current[0]?.parentElement;
      const currentWords = wordsRef.current;
      if (container) {
        karaokeRef.current.activateLine(
          container,
          currentWords!,
          wordElsRef.current,
        );
        karaokeRef.current.setTargetAlpha(0.2, 1.0);
      }
      lyricsLineRuntime.registerKaraoke(lineIndex, karaokeRef.current);
      return;
    }

    lyricsLineRuntime.unregisterKaraoke(lineIndex);

    if (state === "past") {
      karaokeRef.current?.setCurrentAlpha(1.0, 1.0);
      return;
    }

    karaokeRef.current?.setTargetAlpha(0.2, 1.0);
    karaokeRef.current?.deactivateLine();
  }, [lineIndex, shouldUseKaraokeFill, state, activeWordIndex]);

  useEffect(
    () => () => {
      lyricsLineRuntime.unregisterKaraoke(lineIndex);
      karaokeRef.current?.destroy();
      karaokeRef.current = null;
    },
    [lineIndex],
  );

  return (
    <div
      onClick={isSeekable ? handleClick : undefined}
      className={`flex flex-col items-center gap-1.5 text-center ${
        isSeekable ? "cursor-pointer group/line" : ""
      }`}
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

            if (
              shouldEmphasize(word) &&
              isActiveWord &&
              presentation !== "audience"
            ) {
              const wordDuration = word.end_ms - word.time_ms;
              const last = isLastWord(idx, line.words!.length);
              // RATIONALE: Animate the whole word. Splitting into per-character
              // inline-block spans reflows the line on every active-word change
              // (metrics/wrapping shift), which used to yank the scroll target
              // mid-line. Transform/text-shadow on one inline-block word keeps
              // the same box model as the non-emphasis path.
              return (
                <span
                  key={idx}
                  ref={(el) => {
                    if (el) wordElsRef.current[idx] = el;
                  }}
                  className="motion-surface relative inline-block text-white"
                  style={{
                    textShadow:
                      "0 0 12px rgba(255,255,255,0.5), 0 0 4px rgba(255,255,255,0.4)",
                    animation: last
                      ? `lyric-char-glow-last ${wordDuration * 1.2}ms ease-in-out`
                      : `lyric-char-glow ${wordDuration}ms ease-in-out`,
                  }}
                >
                  {word.text}
                  {idx < line.words!.length - 1 ? " " : ""}
                </span>
              );
            }

            return (
              <span
                key={idx}
                ref={(el) => {
                  if (el) wordElsRef.current[idx] = el;
                }}
                className={
                  presentation === "audience"
                    ? "motion-surface"
                    : `motion-surface relative inline-block ${
                        wordState === "active"
                          ? "text-white"
                          : wordState === "past"
                            ? "text-white/45"
                            : "text-white/50"
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
                          textShadow: isLastWord(idx, line.words!.length)
                            ? "0 0 20px rgba(255,255,255,0.7), 0 0 8px rgba(255,255,255,0.5)"
                            : "0 0 12px rgba(255,255,255,0.5), 0 0 4px rgba(255,255,255,0.4)",
                        }
                      : undefined),
                }}
              >
                {word.text}
                {idx < line.words!.length - 1 ? " " : ""}
              </span>
            );
          })}
        </span>
      ) : hasOnlyBackgroundWords ? null : (
        <span
          className={(presentation === "audience"
            ? `motion-surface font-bold tracking-tight ${hoverClass}`
            : `motion-surface ${textSizeClass} ${
                state === "plain" || state === "active"
                  ? "text-white"
                  : state === "past"
                    ? "text-white/45"
                    : "text-white/50"
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
              ? "motion-surface font-medium tracking-tight"
              : `motion-surface text-sm font-medium md:text-base ${
                  state === "plain" || state === "active"
                    ? "text-[var(--color-text-dim)]"
                    : state === "past"
                      ? "text-[var(--color-text-dimmer)]"
                      : "text-[var(--color-text-dim)]"
                }`
          }
          style={{
            ...(presentation === "audience"
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
                }
              : undefined),
            transition: "opacity 0.3s ease, transform 0.3s ease",
            opacity: state === "active" ? 0.4 : 0,
            transform: state === "active" ? "translateY(0)" : "translateY(8px)",
          }}
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
