/** First user-perceived character, so emoji and CJK names render whole. */
export function firstGrapheme(text: string): string {
  const trimmed = text.trim();
  if (!trimmed) return "♪";
  if (typeof Intl !== "undefined" && "Segmenter" in Intl) {
    const segmenter = new Intl.Segmenter(undefined, {
      granularity: "grapheme",
    });
    const first = segmenter.segment(trimmed)[Symbol.iterator]().next();
    if (!first.done) return first.value.segment.toUpperCase();
  }
  return Array.from(trimmed)[0].toUpperCase();
}

/** m:ss for a duration in milliseconds; an em dash when unknown. */
export function formatDuration(durationMs: number | null): string {
  if (durationMs == null || durationMs < 0) return "—";
  const totalSeconds = Math.round(durationMs / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

/** Localized medium date for a millisecond timestamp; null when unknown. */
export function formatUpdated(
  updatedAtMs: number | null | undefined,
  locale?: string,
): string | null {
  if (!updatedAtMs || updatedAtMs <= 0) return null;
  try {
    return new Intl.DateTimeFormat(locale, { dateStyle: "medium" }).format(
      new Date(updatedAtMs),
    );
  } catch {
    return null;
  }
}
