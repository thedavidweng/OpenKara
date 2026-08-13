import type { RefObject } from "react";
import type { AudiencePresentationSpec, LyricLine as Line } from "@/types/ipc";
import type { LyricsAlignment } from "@/lib/lyrics-session";
import { LyricLine } from "./LyricLine";
import type { LyricsPresentation } from "./lyrics-panel-model";

interface PlainTextPageMeasurementLayerProps {
  measurementRef: RefObject<HTMLDivElement | null>;
  lines: Line[];
  presentation: LyricsPresentation;
  lyricsFontStep: number;
  alignment: LyricsAlignment;
  audienceSpec: AudiencePresentationSpec;
  romanizedTextAt: (index: number) => string | undefined;
}

/**
 * An offscreen copy of every line at the audience display's real type size.
 * Paging measures this layer, so page breaks stay correct while the visible
 * viewport only ever holds one page.
 */
export function PlainTextPageMeasurementLayer({
  measurementRef,
  lines,
  presentation,
  lyricsFontStep,
  alignment,
  audienceSpec,
  romanizedTextAt,
}: PlainTextPageMeasurementLayerProps) {
  return (
    <div
      aria-hidden="true"
      className="pointer-events-none absolute inset-0 opacity-0"
    >
      <div
        className="flex h-full w-full"
        style={{
          padding: `${audienceSpec.verticalPaddingPx}px ${audienceSpec.horizontalPaddingPx}px`,
        }}
      >
        <div
          ref={measurementRef}
          className="mx-auto flex w-full flex-col items-center"
          style={{
            maxWidth:
              alignment === "left"
                ? "100%"
                : `min(${audienceSpec.contentWidthRatio * 100}vw, ${audienceSpec.contentMaxWidthPx}px)`,
            gap: audienceSpec.lineGapPx,
          }}
        >
          {lines.map((line, index) => (
            <div
              key={`measure-${index}-${line.time_ms}-${line.text}`}
              data-plain-text-page-measure-line
              className="w-full"
            >
              <LyricLine
                lineIndex={index}
                line={line}
                state="plain"
                presentation={presentation}
                lyricsFontStep={lyricsFontStep}
                romanizedText={romanizedTextAt(index)}
                alignment={alignment}
              />
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
