import { memo, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { KaraokeFillController } from "./karaoke-fill";
import {
  buildAudiencePresentationSpec,
  colorToCss,
} from "@/lib/audience-presentation";
import { lyricsLineRuntime } from "@/lib/lyrics-line-runtime";
import { usePlayerStore } from "@/stores/player-store";
import type { LyricLine as LyricLineType } from "@/types/ipc";
import {
  getLyricsRomanTextSizeClass,
  getLyricsSecondaryTextSizeClass,
  getLyricsTextSizeClass,
  getSeekableHoverClass,
  hasBackgroundWords,
  isLastWord,
  shouldEmphasizeWord,
} from "./lyric-line-typography";
import type { LyricLineState, LyricsPresentation } from "./lyrics-panel-model";

interface LyricLineProps {
  lineIndex: number;
  line: LyricLineType;
  state: LyricLineState;
  activeWordIndex?: number;
  presentation?: LyricsPresentation;
  lyricsFontStep: number;
  romanizedText?: string;
  alignment?: "center" | "left";
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
    previous.romanizedText !== next.romanizedText ||
    previous.alignment !== next.alignment
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
  alignment = "center",
}: LyricLineProps) {
  const { t } = useTranslation();
  const seek = usePlayerStore((s) => s.seek);
  const isSeekable = state !== "plain";
  const isLeftAligned = alignment === "left";
  const textSizeClass = getLyricsTextSizeClass(presentation, lyricsFontStep);
  const secondaryTextSizeClass =
    getLyricsSecondaryTextSizeClass(lyricsFontStep);
  const romanTextSizeClass = getLyricsRomanTextSizeClass(lyricsFontStep);
  const audiencePresentationSpec =
    buildAudiencePresentationSpec(lyricsFontStep);

  const handleClick = () => {
    if (!isSeekable) return;
    void seek(line.time_ms);
  };

  const words = line.words;
  const hasWords = words !== null && words.length > 0;
  const hasOnlyBackgroundWords = !hasWords && hasBackgroundWords(line);
  const shouldUseKaraokeFill =
    state === "active" && hasWords && presentation !== "audience";
  const hoverClass = getSeekableHoverClass(presentation, isSeekable);

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

  const lineClassName = isLeftAligned
    ? `grid w-full ${
        presentation === "audience"
          ? "grid-cols-[minmax(0,1fr)_auto]"
          : "grid-cols-[minmax(0,1fr)_220px]"
      } items-baseline gap-x-8 gap-y-1.5 text-left ${
        isSeekable ? "cursor-pointer group/line" : ""
      }`
    : `flex flex-col items-center gap-1.5 text-center ${
        isSeekable ? "cursor-pointer group/line" : ""
      }`;
  const lineStyle = {
    fontFamily:
      '-apple-system, BlinkMacSystemFont, "Helvetica Neue", "Noto Sans SC", "Noto Sans JP", "Noto Sans KR", system-ui, sans-serif',
    ...(presentation === "audience" && !isLeftAligned
      ? {
          transform:
            state === "active"
              ? `scale(${audiencePresentationSpec.activeScale})`
              : undefined,
        }
      : undefined),
  };

  const mainText = hasWords ? (
    <span
      className={(presentation === "audience"
        ? `tracking-tight min-w-0 break-words ${hoverClass}`
        : `${textSizeClass} min-w-0 break-words ${hoverClass}`
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
          shouldEmphasizeWord(word) &&
          isActiveWord &&
          presentation !== "audience"
        ) {
          const wordDuration = word.end_ms - word.time_ms;
          const last = isLastWord(idx, line.words!.length);
          return (
            <span
              key={idx}
              ref={(el) => {
                if (el) wordElsRef.current[idx] = el;
              }}
              className="motion-surface relative inline-block text-[var(--color-lyrics-active)]"
              style={{
                textShadow: "var(--shadow-lyrics-glow)",
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
                      ? "text-[var(--color-lyrics-active)]"
                      : wordState === "past"
                        ? "text-[var(--color-lyrics-past)]"
                        : "text-[var(--color-lyrics-future)]"
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
                        ? "var(--shadow-lyrics-glow-strong)"
                        : "var(--shadow-lyrics-glow)",
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
        ? `motion-surface font-bold tracking-tight min-w-0 break-words ${hoverClass}`
        : `motion-surface ${textSizeClass} min-w-0 break-words ${
            state === "plain" || state === "active"
              ? "text-[var(--color-lyrics-active)]"
              : state === "past"
                ? "text-[var(--color-lyrics-past)]"
                : "text-[var(--color-lyrics-future)]"
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
  );

  const bgWords =
    line.bg_words && line.bg_words.length > 0 ? (
      <span
        className={
          presentation === "audience"
            ? "motion-surface font-medium tracking-tight min-w-0 break-words"
            : `motion-surface ${secondaryTextSizeClass} font-medium min-w-0 break-words ${
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
    ) : null;

  const romanStateColor =
    state === "plain" || state === "active"
      ? audiencePresentationSpec.activeTextColor
      : state === "past"
        ? audiencePresentationSpec.pastTextColor
        : audiencePresentationSpec.futureTextColor;

  const romanText = romanizedText ? (
    <span
      className={
        isLeftAligned
          ? `motion-surface font-medium tracking-tight min-w-0 break-words ${hoverClass} ${
              presentation === "standard"
                ? `${romanTextSizeClass} ${
                    state === "plain" || state === "active"
                      ? "text-[var(--color-lyrics-active)]"
                      : state === "past"
                        ? "text-[var(--color-lyrics-past)]"
                        : "text-[var(--color-lyrics-future)]"
                  }`
                : ""
            }`
          : presentation === "audience"
            ? "motion-surface font-medium tracking-tight opacity-50"
            : `motion-surface ${secondaryTextSizeClass} font-medium ${
                state === "plain" || state === "active"
                  ? "text-[var(--color-text-dim)]"
                  : state === "past"
                    ? "text-[var(--color-text-dimmer)]"
                    : "text-[var(--color-text-dim)]"
              }`
      }
      style={
        isLeftAligned
          ? {
              ...(presentation === "audience"
                ? {
                    fontSize: audiencePresentationSpec.fontSizePx * 0.7,
                    lineHeight: audiencePresentationSpec.lineHeightMultiple,
                    color: colorToCss(romanStateColor),
                    textShadow:
                      state === "active"
                        ? `0 0 ${audiencePresentationSpec.activeGlowBlurPx}px ${colorToCss(
                            audiencePresentationSpec.activeGlowColor,
                          )}`
                        : undefined,
                    opacity: 0.85,
                  }
                : undefined),
            }
          : presentation === "audience"
            ? {
                fontSize: audiencePresentationSpec.fontSizePx * 0.55,
                lineHeight: audiencePresentationSpec.lineHeightMultiple,
                color: colorToCss(romanStateColor),
                opacity: 0.45,
              }
            : undefined
      }
    >
      {romanizedText}
    </span>
  ) : null;

  const lineContent = (
    <>
      <span
        className={`flex flex-col ${
          isLeftAligned ? "min-w-0 break-words" : "items-center"
        }`}
      >
        {mainText}
        {bgWords}
      </span>
      {romanText}
    </>
  );

  if (isSeekable) {
    const wordText = hasWords
      ? words!
          .map((word) => word.text)
          .join(" ")
          .trim()
      : "";
    const visibleName = line.text.trim() || wordText;
    const ariaLabel = visibleName
      ? undefined
      : t("player.seekToLine", { index: lineIndex + 1 });

    return (
      <button
        type="button"
        onClick={handleClick}
        aria-label={ariaLabel}
        className={`${lineClassName} w-full appearance-none border-0 bg-transparent p-0 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)]`}
        style={lineStyle}
      >
        {lineContent}
      </button>
    );
  }

  return (
    <div className={lineClassName} style={lineStyle}>
      {lineContent}
    </div>
  );
}, areLyricLinePropsEqual);
