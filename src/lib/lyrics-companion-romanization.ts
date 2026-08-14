import { isLatinScript } from "lyric-romanizer/detector";
import type { LyricLine } from "@/types/ipc";

export interface CompanionRomanizationSplit {
  lines: LyricLine[];
  romanizedLines: string[];
  complete: boolean;
}

const MIN_PAIRED_RATIO = 0.5;
const MIN_PAIRED_LINES = 3;

const COMPANION_TIMESTAMP_TOLERANCE_MS = 50;

function isTranscriptionCandidate(line: LyricLine): boolean {
  const text = line.text.trim();
  if (text.length === 0) return false;
  if ((line.words?.length ?? 0) > 0) return false;
  return isLatinScript([text]);
}

export function lineNeedsRomanization(line: LyricLine): boolean {
  const text = line.text.trim();
  return text.length > 0 && !isLatinScript([text]);
}

function sharesTimestamp(primary: LyricLine, candidate: LyricLine): boolean {
  return (
    Math.abs(candidate.time_ms - primary.time_ms) <=
    COMPANION_TIMESTAMP_TOLERANCE_MS
  );
}

function countTimestampPairs(lines: LyricLine[]): number {
  let pairs = 0;
  for (let i = 1; i < lines.length; i += 1) {
    const previous = lines[i - 1];
    const current = lines[i];
    if (
      sharesTimestamp(previous, current) &&
      lineNeedsRomanization(previous) &&
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
  const primaryCount = lines.filter(lineNeedsRomanization).length;
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
      lineNeedsRomanization(previous) &&
      isTranscriptionCandidate(line)
    ) {
      romanizedLines[previousIndex] = line.text.trim();
      continue;
    }

    keptLines.push(line);
    romanizedLines.push("");
  }

  const complete = keptLines.every(
    (line, index) =>
      !lineNeedsRomanization(line) || romanizedLines[index] !== "",
  );

  return { lines: keptLines, romanizedLines, complete };
}
