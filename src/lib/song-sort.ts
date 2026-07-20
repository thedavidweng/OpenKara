import type { LibrarySortMode, Song } from "@/types/ipc";
import { ALPHABET_BUCKETS, bucketForSortKey } from "./alphabet-index";

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

// null/empty/whitespace-only values are treated as missing so they sort after
// present keys.
function normalizeKey(value: string | null): string | null {
  if (value == null) return null;
  const normalized = value.normalize("NFC").trim();
  return normalized.length === 0 ? null : normalized;
}

// Alphabetical sort modes use this as their primary key so every rail bucket
// is contiguous and ordered exactly as the rail is rendered. This matters for
// mixed Han/Latin libraries: Intl.Collator(["zh-Hans-CN", "en"]) groups Han
// text before Latin text, while the rail groups Han by pinyin initial.
function alphabetBucketOrder(value: string | null): number {
  return ALPHABET_BUCKETS.indexOf(bucketForSortKey(value));
}

// Missing keys sort after present keys. Recently-imported mode intentionally
// keeps the legacy locale-only tie-break behavior because it does not show an
// alphabet rail.
function compareTextKeys(
  a: string | null,
  b: string | null,
  useAlphabetBucketOrder: boolean,
): number {
  const aKey = normalizeKey(a);
  const bKey = normalizeKey(b);
  if (aKey == null && bKey == null) return 0;
  if (aKey == null) return 1;
  if (bKey == null) return -1;
  if (useAlphabetBucketOrder) {
    const bucketDiff = alphabetBucketOrder(aKey) - alphabetBucketOrder(bKey);
    if (bucketDiff !== 0) return bucketDiff;
  }
  return collator.compare(aKey, bKey);
}

// non-finite values collapse to 0 so they sort last when descending.
function importedAtDescending(a: number, b: number): number {
  const aMs = Number.isFinite(a) ? a : 0;
  const bMs = Number.isFinite(b) ? b : 0;
  if (aMs === bMs) return 0;
  return bMs - aMs;
}

// Final deterministic tie-break on the raw hash — makes the order a total
// order even when the collator considers the primary/secondary keys equivalent.
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
      const byTitle = compareTextKeys(a.title, b.title, false);
      if (byTitle !== 0) return byTitle;
      const byArtist = compareTextKeys(a.artist, b.artist, false);
      if (byArtist !== 0) return byArtist;
      return compareHash(a.hash, b.hash);
    }
    case "title_asc": {
      const byTitle = compareTextKeys(a.title, b.title, true);
      if (byTitle !== 0) return byTitle;
      const byArtist = compareTextKeys(a.artist, b.artist, false);
      if (byArtist !== 0) return byArtist;
      const byImported = importedAtDescending(a.imported_at, b.imported_at);
      if (byImported !== 0) return byImported;
      return compareHash(a.hash, b.hash);
    }
    case "artist_asc": {
      const byArtist = compareTextKeys(a.artist, b.artist, true);
      if (byArtist !== 0) return byArtist;
      const byTitle = compareTextKeys(a.title, b.title, false);
      if (byTitle !== 0) return byTitle;
      const byImported = importedAtDescending(a.imported_at, b.imported_at);
      if (byImported !== 0) return byImported;
      return compareHash(a.hash, b.hash);
    }
  }
}

// bucketForSortKey is expensive (NFC normalize, Intl.Segmenter grapheme
// iteration, Unicode regexes, pinyin conversion), so alphabetical modes
// compute the primary bucket once per song instead of once per pairwise
// comparison. Recently-imported mode has no rail and avoids this work entirely.
interface DecoratedSong {
  song: Song;
  titleKey: string | null;
  titleBucketOrder: number;
  artistKey: string | null;
  artistBucketOrder: number;
  importedAt: number;
  hash: string;
}

function decorateSong(song: Song, mode: LibrarySortMode): DecoratedSong {
  const titleKey = normalizeKey(song.title);
  const artistKey = normalizeKey(song.artist);
  return {
    song,
    titleKey,
    titleBucketOrder: mode === "title_asc" ? alphabetBucketOrder(titleKey) : 0,
    artistKey,
    artistBucketOrder:
      mode === "artist_asc" ? alphabetBucketOrder(artistKey) : 0,
    importedAt: Number.isFinite(song.imported_at) ? song.imported_at : 0,
    hash: song.hash,
  };
}

// Same logic as compareTextKeys but uses precomputed normalized keys and
// bucket order instead of recomputing them on every comparison.
function compareDecoratedTextKeys(
  aKey: string | null,
  aBucketOrder: number,
  bKey: string | null,
  bBucketOrder: number,
  useAlphabetBucketOrder: boolean,
): number {
  if (aKey == null && bKey == null) return 0;
  if (aKey == null) return 1;
  if (bKey == null) return -1;
  if (useAlphabetBucketOrder) {
    const bucketDiff = aBucketOrder - bBucketOrder;
    if (bucketDiff !== 0) return bucketDiff;
  }
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
        a.titleBucketOrder,
        b.titleKey,
        b.titleBucketOrder,
        false,
      );
      if (byTitle !== 0) return byTitle;
      const byArtist = compareDecoratedTextKeys(
        a.artistKey,
        a.artistBucketOrder,
        b.artistKey,
        b.artistBucketOrder,
        false,
      );
      if (byArtist !== 0) return byArtist;
      return compareHash(a.hash, b.hash);
    }
    case "title_asc": {
      const byTitle = compareDecoratedTextKeys(
        a.titleKey,
        a.titleBucketOrder,
        b.titleKey,
        b.titleBucketOrder,
        true,
      );
      if (byTitle !== 0) return byTitle;
      const byArtist = compareDecoratedTextKeys(
        a.artistKey,
        a.artistBucketOrder,
        b.artistKey,
        b.artistBucketOrder,
        false,
      );
      if (byArtist !== 0) return byArtist;
      if (a.importedAt !== b.importedAt) return b.importedAt - a.importedAt;
      return compareHash(a.hash, b.hash);
    }
    case "artist_asc": {
      const byArtist = compareDecoratedTextKeys(
        a.artistKey,
        a.artistBucketOrder,
        b.artistKey,
        b.artistBucketOrder,
        true,
      );
      if (byArtist !== 0) return byArtist;
      const byTitle = compareDecoratedTextKeys(
        a.titleKey,
        a.titleBucketOrder,
        b.titleKey,
        b.titleBucketOrder,
        false,
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
  // Decorate-sort-undecorate: alphabetical modes precompute their expensive
  // per-song bucket (NFC normalize, Intl.Segmenter, pinyin conversion) once
  // rather than once per pairwise comparison. For 5,000 songs this reduces
  // bucketForSortKey calls from ~120k (O(N log N)) to 5k (O(N)); the recently
  // imported mode performs no bucket conversion because it has no rail.
  const decorated = songs.map((song) => decorateSong(song, mode));
  decorated.sort((a, b) => compareDecorated(a, b, mode));
  return decorated.map((d) => d.song);
}
