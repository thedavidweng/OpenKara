import type { LibrarySortMode, Song } from "@/types/ipc";
import { ALPHABET_BUCKETS, bucketForSortKey } from "./alphabet-index";

export type { LibrarySortMode };

const collator = new Intl.Collator(["zh-Hans-CN", "en"], {
  usage: "sort",
  sensitivity: "base",
  numeric: true,
  ignorePunctuation: false,
});

function normalizeKey(value: string | null): string | null {
  if (value == null) return null;
  const normalized = value.normalize("NFC").trim();
  return normalized.length === 0 ? null : normalized;
}

function alphabetBucketOrder(value: string | null): number {
  return ALPHABET_BUCKETS.indexOf(bucketForSortKey(value));
}

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

function importedAtDescending(a: number, b: number): number {
  const aMs = Number.isFinite(a) ? a : 0;
  const bMs = Number.isFinite(b) ? b : 0;
  if (aMs === bMs) return 0;
  return bMs - aMs;
}

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
  const decorated = songs.map((song) => decorateSong(song, mode));
  decorated.sort((a, b) => compareDecorated(a, b, mode));
  return decorated.map((d) => d.song);
}
