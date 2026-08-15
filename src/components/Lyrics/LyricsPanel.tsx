import { useTranslation } from "react-i18next";
import { LocateFixed } from "lucide-react";
import { Tooltip } from "@/components/Overlay/Tooltip";
import { LyricLine } from "./LyricLine";
import { LyricsEmptyState } from "./LyricsEmptyState";
import { LyricsFontSizeControl } from "./LyricsFontSizeControl";
import { LyricsOffsetControl } from "./LyricsOffsetControl";
import { LyricsPanelToolbar } from "./LyricsPanelToolbar";
import { LyricsStatusMessage } from "./LyricsStatusMessage";
import { PlainTextPageMeasurementLayer } from "./PlainTextPageMeasurementLayer";
import { RemotePlainTextPageControls } from "./RemotePlainTextPageControls";
import {
  useLyricsPanelModel,
  type LyricsPresentation,
} from "./lyrics-panel-model";

interface LyricsPanelProps {
  presentation?: LyricsPresentation;
}

export function LyricsPanel({
  presentation = "standard",
}: LyricsPanelProps = {}) {
  const { t } = useTranslation();
  const model = useLyricsPanelModel(presentation);
  const {
    isAudience,
    spaciousStageLayout,
    audienceSpec,
    songId,
    lines,
    lyricsAlignment,
    lyricsFontStep,
    utilityControlsPinned,
    isPlainText,
    paged,
  } = model;

  if (!songId) {
    return (
      <LyricsStatusMessage
        isAudience={isAudience}
        audienceSpec={audienceSpec}
        className="text-[var(--color-text-dimmer)]"
      >
        {t("lyrics.selectSong")}
      </LyricsStatusMessage>
    );
  }

  if (model.isLoading) {
    return (
      <LyricsStatusMessage
        isAudience={isAudience}
        audienceSpec={audienceSpec}
        className="text-[var(--color-text-dim)]"
      >
        {t("lyrics.loadingLyrics")}
      </LyricsStatusMessage>
    );
  }

  if (lines.length === 0) {
    return <LyricsEmptyState presentation={presentation} />;
  }

  return (
    <div
      className="group relative flex flex-1 flex-col items-center overflow-hidden"
      data-lyrics-visual-variant={
        spaciousStageLayout ? "stage-layout" : "default"
      }
      data-native-lyrics-layout={spaciousStageLayout ? "true" : "false"}
    >
      {isAudience ? null : (
        <>
          <LyricsPanelToolbar
            songId={songId}
            rawLrc={model.rawLrc}
            pinned={utilityControlsPinned}
            showRomanized={model.showRomanized}
            isRomanizing={model.isRomanizing}
            onToggleRomanized={model.toggleRomanized}
            alignment={lyricsAlignment}
            onToggleAlignment={model.toggleLyricsAlignment}
          />
          {model.remotePage.visible ? (
            <RemotePlainTextPageControls remotePage={model.remotePage} />
          ) : null}
        </>
      )}
      <div
        ref={model.containerRef}
        key={songId}
        data-testid="lyrics-scroll-viewport"
        data-lyrics-viewport="true"
        data-preview-lyrics-interactive="true"
        className={`flex w-full flex-1 overflow-y-auto animate-[song-fade-in_var(--motion-duration-slow)_var(--motion-ease-emphasized-out)] ${
          isAudience
            ? ""
            : spaciousStageLayout
              ? "px-16 pt-10 pb-24"
              : "px-12 pt-8 pb-20"
        }`}
        style={{
          overflowAnchor: "none",
          overscrollBehaviorY: "contain",
          ...(isAudience
            ? {
                padding: `${audienceSpec.verticalPaddingPx}px ${audienceSpec.horizontalPaddingPx}px`,
              }
            : undefined),
        }}
      >
        <div
          data-lyrics-stage={
            lyricsAlignment === "center" && !isPlainText ? "focus" : "list"
          }
          className={`mx-auto w-full ${
            lyricsAlignment === "center" && !isPlainText
              ? isAudience
                ? "relative"
                : "relative max-w-4xl"
              : `flex flex-col items-center ${
                  isAudience
                    ? "min-h-full justify-start"
                    : spaciousStageLayout
                      ? lyricsAlignment === "center"
                        ? "max-w-4xl gap-12"
                        : "max-w-4xl gap-9"
                      : "max-w-2xl gap-7"
                }`
          }`}
          style={
            isAudience && (lyricsAlignment === "left" || isPlainText)
              ? {
                  maxWidth:
                    lyricsAlignment === "left"
                      ? "100%"
                      : `min(${audienceSpec.contentWidthRatio * 100}vw, ${audienceSpec.contentMaxWidthPx}px)`,
                  gap: audienceSpec.lineGapPx,
                }
              : isAudience
                ? {
                    maxWidth: `min(${audienceSpec.contentWidthRatio * 100}vw, ${audienceSpec.contentMaxWidthPx}px)`,
                  }
                : undefined
          }
        >
          {isAudience &&
          !paged &&
          (lyricsAlignment === "left" || isPlainText) ? (
            <div className="h-[50vh] w-full shrink-0" />
          ) : null}
          {model.visibleLines.map((line, index) => {
            const absoluteIndex = paged
              ? model.currentPageStart + index
              : index;

            return (
              <div
                key={`${absoluteIndex}-${line.time_ms}-${line.text}`}
                ref={model.registerLineWrapper(absoluteIndex)}
                data-lyrics-line-index={absoluteIndex}
                className="w-full"
              >
                <LyricLine
                  lineIndex={absoluteIndex}
                  line={line}
                  state={model.lineState(absoluteIndex)}
                  presentation={presentation}
                  lyricsFontStep={lyricsFontStep}
                  romanizedText={model.romanizedTextAt(absoluteIndex)}
                  activeWordIndex={model.activeWordIndexAt(absoluteIndex)}
                  alignment={lyricsAlignment}
                />
              </div>
            );
          })}
          {paged || (lyricsAlignment === "center" && !isPlainText) ? null : (
            <div
              className={`w-full shrink-0 ${isAudience ? "h-[50vh]" : "h-[30vh]"}`}
            />
          )}
        </div>
      </div>
      {paged ? (
        <PlainTextPageMeasurementLayer
          measurementRef={model.measurementRef}
          lines={lines}
          presentation={presentation}
          lyricsFontStep={lyricsFontStep}
          alignment={lyricsAlignment}
          audienceSpec={audienceSpec}
          romanizedTextAt={model.romanizedTextAt}
        />
      ) : null}
      {!isAudience && !isPlainText ? (
        <div className="pointer-events-none absolute left-4 top-4 z-10 flex px-0">
          <Tooltip label={t("lyrics.followPlaying")}>
            <button
              type="button"
              data-testid="lyrics-follow-playing"
              data-visible={model.userScrollUnlocked}
              data-preview-lyrics-interactive="true"
              onClick={(event) => {
                model.requestFollow();
                event.currentTarget.blur();
              }}
              aria-label={t("lyrics.followPlaying")}
              className="contextual-reveal pointer-events-auto motion-icon-button rounded-full border border-[var(--color-border-light)] bg-[var(--color-sidebar)] p-2 text-[var(--color-text-dim)] hover:border-[color-mix(in_srgb,var(--color-accent)_28%,var(--color-border-light))] hover:bg-[var(--color-hover)] hover:text-[var(--color-control-primary)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50"
            >
              <LocateFixed size={14} />
            </button>
          </Tooltip>
        </div>
      ) : null}
      {isAudience ? null : (
        <div
          className="pointer-events-none absolute inset-x-0 bottom-0 z-10 flex justify-center px-6 pb-5"
          data-visible={utilityControlsPinned}
        >
          <div className="flex items-center gap-3">
            <LyricsOffsetControl
              className="contextual-reveal pointer-events-auto"
              data-visible={utilityControlsPinned}
              data-preview-lyrics-interactive="true"
            />
            <LyricsFontSizeControl
              className="contextual-reveal pointer-events-auto"
              data-visible={utilityControlsPinned}
              data-preview-lyrics-interactive="true"
            />
          </div>
        </div>
      )}
    </div>
  );
}
