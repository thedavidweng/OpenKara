import { isLatinScript } from "lyric-romanizer/detector";
import type { LyricLine } from "@/types/ipc";

/**
 * Bilingual lyric sources (LrcLib and most embedded tags for Japanese songs)
 * interleave a romaji transcription as its own timestamped line:
 *
 *   [00:00.85]どうでもいいような 夜だけど
 *   [00:00.85]doudemoiiyouna yorudakedo
 *
 * The parser has no notion of a companion line, so both render as full-size
 * peer lyrics — visible even when the Romanized-lyrics toggle is off, and
 * duplicating the app's own on-demand romanization when it is on.
 *
 * This pass lifts those transcriptions out of the lyric list and returns them
 * as a parallel romanization array, so a romaji line can only ever appear as
 * the attached sub-line under its primary line, and only while romanization
 * is enabled.
 */
export interface CompanionRomanizationSplit {
  lines: LyricLine[];
  romanizedLines: string[];
  complete: boolean;
}

/**
 * A file must look overwhelmingly interleaved before any line is reclassified.
 * J-pop lyrics routinely contain genuine English lines, and those must never
 * be swallowed as somebody else's pronunciation guide.
 */
const MIN_PAIRED_RATIO = 0.5;
const MIN_PAIRED_LINES = 3;

/**
 * Sources round timestamps differently (centiseconds vs milliseconds), so a
 * transcription may land a hair off its primary line. Two *distinct* lyrics
 * are never this close, which is what makes the tolerance safe.
 *
 * The tolerance is deliberately tiny. A transcription that borrows a whole
 * different beat's timestamp is indistinguishable — by timestamps, counts, or
 * alternation — from a genuine Latin-script lyric line at that beat, and
 * mistaking the latter for pronunciation would delete a real lyric. Such a
 * stray transcription is therefore left as a lyric line: a visible duplicate
 * in a sloppy source beats silently dropping someone's chorus.
 */
const COMPANION_TIMESTAMP_TOLERANCE_MS = 50;

function isTranscriptionCandidate(line: LyricLine): boolean {
  const text = line.text.trim();
  if (text.length === 0) return false;
  // Word-timed lines carry their own karaoke timing; a transcription shadow
  if ((line.words?.length ?? 0) > 0) return false;
  return isLatinScript([text]);
}

function needsRomanization(line: LyricLine): boolean {
  const text = line.text.trim();
  return text.length > 0 && !isLatinScript([text]);
}

function sharesTimestamp(primary: LyricLine, candidate: LyricLine): boolean {
  return (
    Math.abs(candidate.time_ms - primary.time_ms) <=
    COMPANION_TIMESTAMP_TOLERANCE_MS
  );
}

/**
 * Count primary/transcription pairs printed at the same timestamp. This is the
 * unambiguous signature of an interleaved bilingual file: two lyric lines at
 * the very same moment, one non-Latin and one Latin.
 */
function countTimestampPairs(lines: LyricLine[]): number {
  let pairs = 0;
  for (let i = 1; i < lines.length; i += 1) {
    const previous = lines[i - 1];
    const current = lines[i];
    if (
      sharesTimestamp(previous, current) &&
      needsRomanization(previous) &&
      isTranscriptionCandidate(current)
    ) {
      pairs += 1;
    }
  }
  return pairs;
}

export function splitCompanionRomanization(
  lines: LyricLine[],
): CompanionRomanizationSplit {
  const primaryCount = lines.filter(needsRomanization).length;
  const pairCount = countTimestampPairs(lines);

  if (
    pairCount < MIN_PAIRED_LINES ||
    primaryCount === 0 ||
    pairCount < primaryCount * MIN_PAIRED_RATIO
  ) {
    return {
      lines,
      romanizedLines: [],
      complete: false,
    };
  }

  const keptLines: LyricLine[] = [];
  const romanizedLines: string[] = [];

  for (const line of lines) {
    const previousIndex = keptLines.length - 1;
    const previous = previousIndex >= 0 ? keptLines[previousIndex] : null;
    const previousHasRomanization =
      previousIndex >= 0 && romanizedLines[previousIndex] !== "";

    if (
      previous !== null &&
      !previousHasRomanization &&
      sharesTimestamp(previous, line) &&
      needsRomanization(previous) &&
      isTranscriptionCandidate(line)
    ) {
      romanizedLines[previousIndex] = line.text.trim();
      continue;
    }

    keptLines.push(line);
    romanizedLines.push("");
  }

  const complete = keptLines.every(
    (line, index) => !needsRomanization(line) || romanizedLines[index] !== "",
  );

  return { lines: keptLines, romanizedLines, complete };
}
