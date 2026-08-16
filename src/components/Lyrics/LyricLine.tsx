import { memo, useRef, useEffect, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import {
  ACTIVE_BRIGHT_ALPHA,
  ACTIVE_DARK_ALPHA,
  KaraokeFillController,
} from "./karaoke-fill";
import {
  buildAudiencePresentationSpec,
  colorToCss,
} from "@/lib/audience-presentation";
import { lyricsLineRuntime } from "@/lib/lyrics-line-runtime";
import { useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";
import type { LyricLine as LyricLineType } from "@/types/ipc";
import {
  CENTERED_BG_FONT_SIZE,
  CENTERED_LINE_LETTER_SPACING,
  CENTERED_LINE_LINE_HEIGHT,
  CENTERED_LINE_PADDING,
  CENTERED_LINE_FONT_WEIGHT,
  CENTERED_ROMAN_FONT_SIZE,
  STANDARD_LINE_FONT_WEIGHT,
  displayWordText,
  getCenteredLineFontSize,
  getLyricsRomanTextSizeClass,
  getLyricsSecondaryTextSizeClass,
  getLyricsTextSizeClass,
  getSeekableHoverClass,
  hasBackgroundWords,
  isLastWord,
  shouldEmphasizeWord,
  wordTokenGap,
} from "./lyric-line-typography";
import { resolveWordRomans } from "./lyric-word-romans";
import { visibleRomanizedText } from "@/lib/lyrics-roman-visibility";
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

function lineHighlightState(
  state: LyricLineState,
): "active" | "past" | "future" {
  if (state === "plain" || state === "active") {
    return "active";
  }
  return state;
}

function lineStateColorClass(state: LyricLineState): string {
  const highlight = lineHighlightState(state);
  if (highlight === "active") {
    return "text-[var(--color-lyrics-active)]";
  }
  if (highlight === "past") {
    return "text-[var(--color-lyrics-past)]";
  }
  return "text-[var(--color-lyrics-future)]";
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

  const usesOverlayFill =
    previous.presentation !== "audience" &&
    previous.line.words !== null &&
    previous.line.words.length > 0;
  const hasEmphasis = previous.line.words?.some((word) =>
    shouldEmphasizeWord(word),
  );
  if (usesOverlayFill && !hasEmphasis) {
    return true;
  }

  return (previous.activeWordIndex ?? -1) === (next.activeWordIndex ?? -1);
}

export const LyricLine = memo(function LyricLine({
  lineIndex,
  line,
  state,
  activeWordIndex: activeWordIndexProp,
  presentation = "standard",
  lyricsFontStep,
  romanizedText,
  alignment = "center",
}: LyricLineProps) {
  const { t } = useTranslation();
  const liveWordIndex = useLyricsStore((store) =>
    store.activeLineIndex === lineIndex ? store.activeWordIndex : -1,
  );
  const activeWordIndex = activeWordIndexProp ?? liveWordIndex;
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
  const displayRomanizedText = visibleRomanizedText(
    line.text.trim() ||
      (hasWords ? words.map((word) => word.text).join(" ") : ""),
    romanizedText,
  );
  const wordRomans = displayRomanizedText
    ? resolveWordRomans(words, displayRomanizedText)
    : null;
  const hideLineRoman = !isLeftAligned && wordRomans !== null;
  const hasOnlyBackgroundWords = !hasWords && hasBackgroundWords(line);
  const hasWordMask = hasWords && presentation !== "audience";
  const shouldUseKaraokeFill = state === "active" && hasWordMask;
  const hoverClass = getSeekableHoverClass(presentation, isSeekable);

  const karaokeRef = useRef<KaraokeFillController | null>(null);
  const wordElsRef = useRef<HTMLElement[]>([]);
  const romanElsRef = useRef<Array<HTMLElement | null>>([]);
  const wordsRef = useRef(words);
  wordsRef.current = words;

  useEffect(() => {
    if (shouldUseKaraokeFill) {
      if (!karaokeRef.current) {
        karaokeRef.current = new KaraokeFillController();
      }
      const container = wordElsRef.current[0]?.closest("[data-karaoke-line]");
      const currentWords = wordsRef.current;
      if (container instanceof HTMLElement && currentWords) {
        karaokeRef.current.activateLine(
          container,
          currentWords,
          wordElsRef.current,
          romanElsRef.current,
        );
      }
      lyricsLineRuntime.registerKaraoke(lineIndex, karaokeRef.current);
      return;
    }

    lyricsLineRuntime.unregisterKaraoke(lineIndex);
    karaokeRef.current?.deactivateLine();
  }, [lineIndex, shouldUseKaraokeFill]);

  useEffect(
    () => () => {
      lyricsLineRuntime.unregisterKaraoke(lineIndex);
      karaokeRef.current?.destroy();
      karaokeRef.current = null;
    },
    [lineIndex],
  );

  const usesCenteredLineType = !isLeftAligned && presentation !== "audience";
  const showLineRoman = Boolean(displayRomanizedText && !hideLineRoman);
  const lineClassName = isLeftAligned
    ? `grid w-full ${
        showLineRoman
          ? presentation === "audience"
            ? "grid-cols-[minmax(0,1fr)_auto]"
            : "grid-cols-[minmax(0,1fr)_220px]"
          : "grid-cols-1"
      } items-baseline gap-x-8 gap-y-1.5 text-left ${
        isSeekable ? "cursor-pointer group/line" : ""
      }`
    : `flex flex-col items-center gap-[0.28em] text-center ${
        isSeekable ? "cursor-pointer group/line" : ""
      }`;
  const lineStyle = {
    fontFamily: "inherit",
    ...(usesCenteredLineType
      ? {
          fontSize: getCenteredLineFontSize(lyricsFontStep),
          lineHeight: CENTERED_LINE_LINE_HEIGHT,
          letterSpacing: CENTERED_LINE_LETTER_SPACING,
          padding: CENTERED_LINE_PADDING,
          fontWeight: CENTERED_LINE_FONT_WEIGHT,
          textWrap: "pretty" as const,
        }
      : undefined),
    ...(shouldUseKaraokeFill
      ? ({
          "--bright-mask-alpha": String(ACTIVE_BRIGHT_ALPHA),
          "--dark-mask-alpha": String(ACTIVE_DARK_ALPHA),
        } as CSSProperties)
      : undefined),
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
      data-karaoke-line={shouldUseKaraokeFill ? "true" : undefined}
      className={(presentation === "audience"
        ? `tracking-tight min-w-0 break-words ${hoverClass}`
        : usesCenteredLineType
          ? `tracking-tight min-w-0 break-words ${hoverClass}`
          : `${textSizeClass} min-w-0 break-words ${hoverClass}`
      ).trim()}
      style={{
        fontWeight: usesCenteredLineType
          ? "inherit"
          : STANDARD_LINE_FONT_WEIGHT,
        ...(presentation === "audience"
          ? {
              fontSize: audiencePresentationSpec.fontSizePx,
              lineHeight: audiencePresentationSpec.lineHeightMultiple,
            }
          : undefined),
      }}
    >
      {line.words!.map((word, idx) => {
        const wordState = isLeftAligned
          ? lineHighlightState(state)
          : state === "plain"
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

        const gap = wordTokenGap(word.text, line.words![idx + 1]?.text);
        const wordRoman = wordRomans?.[idx] ?? null;
        const emphasize =
          state === "active" &&
          shouldEmphasizeWord(word) &&
          (idx <= activeWordIndex || activeWordIndex < 0);
        const emphasizeClass = emphasize
          ? isLastWord(idx, line.words!.length)
            ? "lyric-word-emphasize lyric-word-emphasize-last"
            : "lyric-word-emphasize"
          : "";
        const emphasizeStyle = emphasize
          ? ({
              "--lyric-emp-ms": `${Math.max(1000, word.end_ms - word.time_ms)}ms`,
            } as CSSProperties)
          : undefined;
        const glyph = displayWordText(word.text);
        const romanNode =
          !isLeftAligned && wordRoman ? (
            <span
              ref={(el) => {
                romanElsRef.current[idx] = el;
              }}
              data-word-roman="true"
              data-karaoke-roman-fill={
                shouldUseKaraokeFill ? "true" : undefined
              }
              className="inline-block font-medium tracking-wide text-[var(--color-lyrics-active)]"
              style={{
                fontSize: "max(0.5em, 10px)",
                lineHeight: 1.1,
                textAlign: "center",
                whiteSpace: "nowrap",
                opacity: state === "plain" || state === "active" ? 0.5 : 0.3,
              }}
            >
              {wordRoman}
            </span>
          ) : null;

        if (shouldUseKaraokeFill) {
          return (
            <span key={idx}>
              <span className="inline-flex flex-col items-center align-bottom">
                <span
                  ref={(el) => {
                    if (el) wordElsRef.current[idx] = el;
                  }}
                  data-karaoke-fill="true"
                  className="motion-surface relative inline-block text-[var(--color-lyrics-active)]"
                >
                  <span className={emphasizeClass} style={emphasizeStyle}>
                    {glyph}
                  </span>
                </span>
                {romanNode}
              </span>
              {gap}
            </span>
          );
        }

        return (
          <span key={idx}>
            <span
              className={
                presentation === "audience"
                  ? "motion-surface inline-flex flex-col items-center align-bottom"
                  : `motion-surface relative inline-flex flex-col items-center align-bottom ${
                      isLeftAligned
                        ? lineStateColorClass(state)
                        : wordState === "active"
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
                  : undefined),
              }}
            >
              <span className={emphasizeClass} style={emphasizeStyle}>
                {glyph}
              </span>
              {romanNode}
            </span>
            {gap}
          </span>
        );
      })}
    </span>
  ) : hasOnlyBackgroundWords ? null : (
    <span
      className={(presentation === "audience"
        ? `motion-surface font-bold tracking-tight min-w-0 break-words ${hoverClass}`
        : `motion-surface ${
            usesCenteredLineType ? "tracking-tight" : textSizeClass
          } min-w-0 break-words ${lineStateColorClass(state)} ${hoverClass}`
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
        data-lyrics-bg="true"
        className={
          presentation === "audience"
            ? "motion-surface font-medium tracking-tight min-w-0 break-words"
            : usesCenteredLineType
              ? "motion-surface font-medium min-w-0 break-words text-[var(--color-lyrics-active)]"
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
            : usesCenteredLineType
              ? {
                  fontSize: CENTERED_BG_FONT_SIZE,
                }
              : undefined),
          transition: "opacity 0.3s ease, transform 0.3s ease",
          opacity: state === "active" ? 0.4 : 0,
          transform: state === "active" ? "translateY(0)" : "translateY(0.4em)",
          visibility: state === "active" ? "visible" : "hidden",
          height: state === "active" ? "auto" : 0,
          overflow: "hidden",
          pointerEvents: state === "active" ? "auto" : "none",
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

  const romanText =
    displayRomanizedText && !hideLineRoman ? (
      <span
        data-lyrics-roman="true"
        className={
          isLeftAligned
            ? `motion-surface font-medium tracking-tight min-w-0 break-words ${hoverClass} ${
                presentation === "standard"
                  ? `${romanTextSizeClass} ${lineStateColorClass(state)}`
                  : ""
              }`
            : presentation === "audience"
              ? "motion-surface font-medium tracking-tight opacity-50"
              : "motion-surface font-medium tracking-wide text-[var(--color-lyrics-active)]"
        }
        style={
          isLeftAligned
            ? {
                fontWeight: STANDARD_LINE_FONT_WEIGHT,
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
              : {
                  fontSize: CENTERED_ROMAN_FONT_SIZE,
                  lineHeight: 1.5,
                  opacity: state === "plain" || state === "active" ? 0.5 : 0.3,
                }
        }
      >
        {isLeftAligned && wordRomans
          ? wordRomans.map((wordRoman, idx) => {
              if (!wordRoman) {
                return null;
              }
              return (
                <span key={idx}>
                  <span
                    ref={(el) => {
                      romanElsRef.current[idx] = el;
                    }}
                    data-karaoke-roman-fill={
                      shouldUseKaraokeFill ? "true" : undefined
                    }
                    className="inline-block"
                  >
                    {wordRoman}
                  </span>
                  {idx < wordRomans.length - 1 ? " " : ""}
                </span>
              );
            })
          : displayRomanizedText}
      </span>
    ) : null;

  const lineContent = isLeftAligned ? (
    <>
      <span className="flex min-w-0 flex-col break-words">
        {mainText}
        {bgWords}
      </span>
      {romanText}
    </>
  ) : (
    <span className="flex flex-col items-center gap-1">
      {mainText}
      {romanText}
      {bgWords}
    </span>
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
