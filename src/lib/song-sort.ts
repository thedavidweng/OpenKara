import type { LibrarySortMode, Song } from "@/types/ipc";

export type { LibrarySortMode };

// One module-level collator reused across every comparison. The locale list
// prioritizes Simplified Chinese pinyin ordering before Latin so mixed
// libraries sort intuitively for the primary audience. `numeric: true` makes
// "Track 2" sort before "Track 10".
const collator = new Intl.Collator(["zh-Hans-CN", "en"], {
  usage: "sort",
  sensitivity: "base",
  numeric: true,
  ignorePunctuation: false,
});

// Normalize a text key by Unicode NFC plus trim(). null/empty/whitespace-only
// values are treated as missing so they sort after present keys.
function normalizeKey(value: string | null): string | null {
  if (value == null) return null;
  const normalized = value.normalize("NFC").trim();
  return normalized.length === 0 ? null : normalized;
}

// Compare two optional text keys. Missing keys sort after present keys. When
// both are present the shared collator decides. When both are missing the
// caller's tie-break chain takes over (returns 0).
function compareTextKeys(a: string | null, b: string | null): number {
  const aKey = normalizeKey(a);
  const bKey = normalizeKey(b);
  if (aKey == null && bKey == null) return 0;
  if (aKey == null) return 1;
  if (bKey == null) return -1;
  return collator.compare(aKey, bKey);
}

// finite imported_at descending; non-finite values collapse to 0 so they sort
// last when descending (older effective import time).
function importedAtDescending(a: number, b: number): number {
  const aMs = Number.isFinite(a) ? a : 0;
  const bMs = Number.isFinite(b) ? b : 0;
  if (aMs === bMs) return 0;
  return bMs - aMs;
}

// Final deterministic tie-break on the raw hash using code-point comparison.
// This makes the order a total order even when the collator considers the
// primary/secondary keys equivalent.
function compareHash(a: string, b: string): number {
  if (a < b) return -1;
  if (a > b) return 1;
  return 0;
}

export function compareSongs(a: Song, b: Song, mode: LibrarySortMode): number {
  switch (mode) {
    case "recently_imported": {
      const byImported = importedAtDescending(a.imported_at, b.imported_at);
      if (byImported !== 0) return byImported;
      const byTitle = compareTextKeys(a.title, b.title);
      if (byTitle !== 0) return byTitle;
      const byArtist = compareTextKeys(a.artist, b.artist);
      if (byArtist !== 0) return byArtist;
      return compareHash(a.hash, b.hash);
    }
    case "title_asc": {
      const byTitle = compareTextKeys(a.title, b.title);
      if (byTitle !== 0) return byTitle;
      const byArtist = compareTextKeys(a.artist, b.artist);
      if (byArtist !== 0) return byArtist;
      const byImported = importedAtDescending(a.imported_at, b.imported_at);
      if (byImported !== 0) return byImported;
      return compareHash(a.hash, b.hash);
    }
    case "artist_asc": {
      const byArtist = compareTextKeys(a.artist, b.artist);
      if (byArtist !== 0) return byArtist;
      const byTitle = compareTextKeys(a.title, b.title);
      if (byTitle !== 0) return byTitle;
      const byImported = importedAtDescending(a.imported_at, b.imported_at);
      if (byImported !== 0) return byImported;
      return compareHash(a.hash, b.hash);
    }
  }
}

export function sortSongs(
  songs: readonly Song[],
  mode: LibrarySortMode,
): Song[] {
  return [...songs].sort((a, b) => compareSongs(a, b, mode));
}
