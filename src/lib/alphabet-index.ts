import { pinyin } from "pinyin-pro";
import type { Song } from "@/types/ipc";
import type { LibrarySortMode } from "./song-sort";

export const ALPHABET_BUCKETS = [
  "A",
  "B",
  "C",
  "D",
  "E",
  "F",
  "G",
  "H",
  "I",
  "J",
  "K",
  "L",
  "M",
  "N",
  "O",
  "P",
  "Q",
  "R",
  "S",
  "T",
  "U",
  "V",
  "W",
  "X",
  "Y",
  "Z",
  "#",
] as const;

export type AlphabetBucket = (typeof ALPHABET_BUCKETS)[number];

// Module-level Intl.Segmenter for grapheme segmentation (handles supplementary
// plane characters that UTF-16 indexing would split).
const GRAPHEME_SEGMENTER = new Intl.Segmenter("und", {
  granularity: "grapheme",
});

const HAN_REGEX = /\p{Script=Han}/u;
const MARK_REGEX = /\p{Mark}/gu;
const ASCII_LETTER_REGEX = /^[A-Z]$/;

/**
 * Map a sort key (title or artist) to one of the 27 alphabet buckets.
 *
 * 1. NFC normalize and trim Unicode whitespace.
 * 2. Segment the first grapheme.
 * 3. Empty/missing → `#`.
 * 4. Han grapheme → pinyin first initial, uppercase, A–Z only; else `#`.
 * 5. Non-Han grapheme → NFD strip marks, uppercase, first code point A–Z; else `#`.
 */
export function bucketForSortKey(value: string | null): AlphabetBucket {
  if (value == null) return "#";

  const normalized = value.normalize("NFC").trim();
  if (normalized.length === 0) return "#";

  const firstGrapheme = GRAPHEME_SEGMENTER.segment(normalized)
    [Symbol.iterator]()
    .next();

  if (firstGrapheme.done) return "#";

  const grapheme = firstGrapheme.value.segment;
  if (grapheme.length === 0) return "#";

  if (HAN_REGEX.test(grapheme)) {
    const initial = pinyin(grapheme, { pattern: "first", toneType: "none" });
    if (!initial) return "#";
    const codePoint = initial.codePointAt(0);
    if (codePoint === undefined) return "#";
    const upper = String.fromCodePoint(codePoint).toUpperCase();
    if (ASCII_LETTER_REGEX.test(upper)) return upper as AlphabetBucket;
    return "#";
  }

  const decomposed = grapheme.normalize("NFD").replace(MARK_REGEX, "");
  if (decomposed.length === 0) return "#";
  const codePoint = decomposed.codePointAt(0);
  if (codePoint === undefined) return "#";
  const upper = String.fromCodePoint(codePoint).toUpperCase();
  if (ASCII_LETTER_REGEX.test(upper)) return upper as AlphabetBucket;
  return "#";
}

export function buildAlphabetIndex(
  songs: readonly Song[],
  mode: "title_asc" | "artist_asc",
): ReadonlyMap<AlphabetBucket, number> {
  const indexByBucket = new Map<AlphabetBucket, number>();
  for (let i = 0; i < songs.length; i++) {
    const song = songs[i];
    const key = mode === "title_asc" ? song.title : song.artist;
    const bucket = bucketForSortKey(key);
    if (!indexByBucket.has(bucket)) {
      indexByBucket.set(bucket, i);
    }
  }
  return indexByBucket;
}

/**
 * Resolve a requested bucket to a song index using the missing-letter fallback:
 * 1. exact mapped index when present;
 * 2. nearest mapped bucket by distance in ALPHABET_BUCKETS order (ties broken
 *    toward the following bucket so the user lands on the next section, not
 *    `#`, when pressing a late letter with no songs);
 * 3. null when the map is empty.
 *
 * Returns both the resolved song index and the bucket that was actually used
 * (for visual/ARIA state), or null when no navigation is possible.
 */
export function resolveBucket(
  indexByBucket: ReadonlyMap<AlphabetBucket, number>,
  requested: AlphabetBucket,
): { index: number; bucket: AlphabetBucket } | null {
  if (indexByBucket.size === 0) return null;

  const exact = indexByBucket.get(requested);
  if (exact !== undefined) {
    return { index: exact, bucket: requested };
  }

  const requestedPos = ALPHABET_BUCKETS.indexOf(requested);

  for (let d = 1; d < ALPHABET_BUCKETS.length; d++) {
    const followingPos = requestedPos + d;
    if (followingPos < ALPHABET_BUCKETS.length) {
      const bucket = ALPHABET_BUCKETS[followingPos];
      const idx = indexByBucket.get(bucket);
      if (idx !== undefined) return { index: idx, bucket };
    }
    const precedingPos = requestedPos - d;
    if (precedingPos >= 0) {
      const bucket = ALPHABET_BUCKETS[precedingPos];
      const idx = indexByBucket.get(bucket);
      if (idx !== undefined) return { index: idx, bucket };
    }
  }

  return null;
}

export type { LibrarySortMode };
