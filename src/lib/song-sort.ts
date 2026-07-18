import type { LibrarySortMode, Song } from "@/types/ipc";
import { bucketForSortKey } from "./alphabet-index";

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

// Bucket rank: A–Z entries get rank 0, "#" (non-alphabetic) entries get rank 1.
// This ensures non-alphabetic songs sort after all lettered songs, matching
// the alphabet rail where "#" is the last marker. Without this, Intl.Collator
// with numeric:true places digit/punctuation-leading titles before letters,
// so clicking the bottom "#" rail marker would scroll to the top of the list.
function bucketRank(value: string | null): number {
  return bucketForSortKey(value) === "#" ? 1 : 0;
}

// Compare two optional text keys. Missing keys sort after present keys. When
// both are present, first compare by bucket rank (letters before "#") so the
// list order matches the rail order, then by the shared collator. When both
// are missing the caller's tie-break chain takes over (returns 0).
function compareTextKeys(a: string | null, b: string | null): number {
  const aKey = normalizeKey(a);
  const bKey = normalizeKey(b);
  if (aKey == null && bKey == null) return 0;
  if (aKey == null) return 1;
  if (bKey == null) return -1;
  const rankDiff = bucketRank(aKey) - bucketRank(bKey);
  if (rankDiff !== 0) return rankDiff;
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

// Precomputed sort fields for a song. bucketForSortKey is expensive (NFC
// normalize, Intl.Segmenter grapheme iteration, Unicode regexes, pinyin
// conversion) so the rank and normalized key are computed once per song in
// sortSongs instead of once per pairwise comparison during sorting.
interface DecoratedSong {
  song: Song;
  titleKey: string | null;
  titleBucketRank: number;
  artistKey: string | null;
  artistBucketRank: number;
  importedAt: number;
  hash: string;
}

function decorateSong(song: Song): DecoratedSong {
  const titleKey = normalizeKey(song.title);
  const artistKey = normalizeKey(song.artist);
  return {
    song,
    titleKey,
    titleBucketRank: bucketRank(titleKey),
    artistKey,
    artistBucketRank: bucketRank(artistKey),
    importedAt: Number.isFinite(song.imported_at) ? song.imported_at : 0,
    hash: song.hash,
  };
}

// Compare two precomputed text keys. Same logic as compareTextKeys but uses
// the already-computed normalized key and bucket rank instead of recomputing
// them on every comparison.
function compareDecoratedTextKeys(
  aKey: string | null,
  aRank: number,
  bKey: string | null,
  bRank: number,
): number {
  if (aKey == null && bKey == null) return 0;
  if (aKey == null) return 1;
  if (bKey == null) return -1;
  const rankDiff = aRank - bRank;
  if (rankDiff !== 0) return rankDiff;
  return collator.compare(aKey, bKey);
}

function compareDecorated(
  a: DecoratedSong,
  b: DecoratedSong,
  mode: LibrarySortMode,
): number {
  switch (mode) {
    case "recently_imported": {
      if (a.importedAt !== b.importedAt) return b.importedAt - a.importedAt;
      const byTitle = compareDecoratedTextKeys(
        a.titleKey,
        a.titleBucketRank,
        b.titleKey,
        b.titleBucketRank,
      );
      if (byTitle !== 0) return byTitle;
      const byArtist = compareDecoratedTextKeys(
        a.artistKey,
        a.artistBucketRank,
        b.artistKey,
        b.artistBucketRank,
      );
      if (byArtist !== 0) return byArtist;
      return compareHash(a.hash, b.hash);
    }
    case "title_asc": {
      const byTitle = compareDecoratedTextKeys(
        a.titleKey,
        a.titleBucketRank,
        b.titleKey,
        b.titleBucketRank,
      );
      if (byTitle !== 0) return byTitle;
      const byArtist = compareDecoratedTextKeys(
        a.artistKey,
        a.artistBucketRank,
        b.artistKey,
        b.artistBucketRank,
      );
      if (byArtist !== 0) return byArtist;
      if (a.importedAt !== b.importedAt) return b.importedAt - a.importedAt;
      return compareHash(a.hash, b.hash);
    }
    case "artist_asc": {
      const byArtist = compareDecoratedTextKeys(
        a.artistKey,
        a.artistBucketRank,
        b.artistKey,
        b.artistBucketRank,
      );
      if (byArtist !== 0) return byArtist;
      const byTitle = compareDecoratedTextKeys(
        a.titleKey,
        a.titleBucketRank,
        b.titleKey,
        b.titleBucketRank,
      );
      if (byTitle !== 0) return byTitle;
      if (a.importedAt !== b.importedAt) return b.importedAt - a.importedAt;
      return compareHash(a.hash, b.hash);
    }
  }
}

export function sortSongs(
  songs: readonly Song[],
  mode: LibrarySortMode,
): Song[] {
  // Decorate-sort-undecorate: precompute the expensive per-song fields (NFC
  // normalize, Intl.Segmenter grapheme iteration, pinyin conversion) once per
  // song instead of once per pairwise comparison. For a 5,000-song library
  // this reduces bucketForSortKey calls from ~120k (O(N log N)) to 10k (O(N)).
  const decorated = songs.map(decorateSong);
  decorated.sort((a, b) => compareDecorated(a, b, mode));
  return decorated.map((d) => d.song);
}
