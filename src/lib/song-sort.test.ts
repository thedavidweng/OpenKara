import { describe, expect, test } from "vitest";
import { compareSongs, sortSongs } from "./song-sort";
import { buildAlphabetIndex, bucketForSortKey } from "./alphabet-index";
import type { Song } from "@/types/ipc";

function makeSong(overrides: Partial<Song> & { hash: string }): Song {
  return {
    file_path: null,
    audio_source_kind: "original",
    cdg_path: null,
    media_g_container: null,
    instrumental: false,
    language: null,
    title: null,
    artist: null,
    album: null,
    duration_ms: 0,
    cover_art: null,
    has_cover_art: false,
    artwork_thumb_path: null,
    imported_at: 0,
    original_ext: null,
    ...overrides,
  };
}

describe("sortSongs / compareSongs", () => {
  test("recently_imported sorts by imported_at descending", () => {
    const songs = [
      makeSong({ hash: "a", imported_at: 1000, title: "Old" }),
      makeSong({ hash: "b", imported_at: 3000, title: "New" }),
      makeSong({ hash: "c", imported_at: 2000, title: "Mid" }),
    ];
    const sorted = sortSongs(songs, "recently_imported");
    expect(sorted.map((s) => s.hash)).toEqual(["b", "c", "a"]);
  });

  test("recently_imported ties break by title then artist then hash", () => {
    const songs = [
      makeSong({ hash: "zzz", imported_at: 1000, title: "Beta", artist: "Z" }),
      makeSong({ hash: "aaa", imported_at: 1000, title: "Alpha", artist: "A" }),
      makeSong({ hash: "mmm", imported_at: 1000, title: "Alpha", artist: "B" }),
    ];
    const sorted = sortSongs(songs, "recently_imported");
    expect(sorted.map((s) => s.hash)).toEqual(["aaa", "mmm", "zzz"]);
  });

  test("title_asc sorts by title ascending", () => {
    const songs = [
      makeSong({ hash: "a", title: "Zebra", imported_at: 9000 }),
      makeSong({ hash: "b", title: "Apple", imported_at: 1000 }),
      makeSong({ hash: "c", title: "Mango", imported_at: 5000 }),
    ];
    const sorted = sortSongs(songs, "title_asc");
    expect(sorted.map((s) => s.hash)).toEqual(["b", "c", "a"]);
  });

  test("title_asc ties break by artist then imported_at descending then hash", () => {
    const songs = [
      makeSong({ hash: "z", title: "Same", artist: "Z", imported_at: 1000 }),
      makeSong({ hash: "a", title: "Same", artist: "A", imported_at: 5000 }),
      makeSong({ hash: "m", title: "Same", artist: "A", imported_at: 1000 }),
    ];
    const sorted = sortSongs(songs, "title_asc");
    expect(sorted.map((s) => s.hash)).toEqual(["a", "m", "z"]);
  });

  test("artist_asc sorts by artist then title", () => {
    const songs = [
      makeSong({ hash: "a", artist: "Zed", title: "Apple" }),
      makeSong({ hash: "b", artist: "Alpha", title: "Zebra" }),
      makeSong({ hash: "c", artist: "Alpha", title: "Apple" }),
    ];
    const sorted = sortSongs(songs, "artist_asc");
    expect(sorted.map((s) => s.hash)).toEqual(["c", "b", "a"]);
  });

  test("artist_asc ties break by imported_at descending then hash", () => {
    const songs = [
      makeSong({ hash: "z", artist: "Same", title: "Same", imported_at: 1000 }),
      makeSong({ hash: "a", artist: "Same", title: "Same", imported_at: 5000 }),
      makeSong({ hash: "m", artist: "Same", title: "Same", imported_at: 5000 }),
    ];
    const sorted = sortSongs(songs, "artist_asc");
    expect(sorted.map((s) => s.hash)).toEqual(["a", "m", "z"]);
  });

  test("null/empty/whitespace keys sort after present keys", () => {
    const songs = [
      makeSong({ hash: "z", title: null, artist: null }),
      makeSong({ hash: "a", title: "  ", artist: "" }),
      makeSong({ hash: "b", title: "Real", artist: "Real" }),
    ];
    const sorted = sortSongs(songs, "title_asc");
    expect(sorted[0].hash).toBe("b");
    // The two missing-title songs tie on title; with no artist either, hash
    // decides: "a" before "z".
    expect(sorted.map((s) => s.hash)).toEqual(["b", "a", "z"]);
  });

  test("NFC-equivalent strings use hash tie-break when collator says equal", () => {
    // é (U+00E9) and e + combining acute (U+0065 U+0301) are NFC-equivalent.
    const songs = [
      makeSong({ hash: "b", title: "e\u0301toile" }),
      makeSong({ hash: "a", title: "étoile" }),
    ];
    const sorted = sortSongs(songs, "title_asc");
    expect(sorted.map((s) => s.hash)).toEqual(["a", "b"]);
  });

  test("case and diacritic equivalence fall back to hash tie-break", () => {
    const songs = [
      makeSong({ hash: "b", title: "cafe" }),
      makeSong({ hash: "a", title: "Café" }),
    ];
    const sorted = sortSongs(songs, "title_asc");
    expect(sorted.map((s) => s.hash)).toEqual(["a", "b"]);
  });

  test("numeric titles sort Track 2 before Track 10", () => {
    const songs = [
      makeSong({ hash: "a", title: "Track 10" }),
      makeSong({ hash: "b", title: "Track 2" }),
    ];
    const sorted = sortSongs(songs, "title_asc");
    expect(sorted.map((s) => s.hash)).toEqual(["b", "a"]);
  });

  test("non-alphabetic titles sort after lettered titles (matches # rail position)", () => {
    const songs = [
      makeSong({ hash: "num", title: "99 Luftballons" }),
      makeSong({ hash: "z", title: "Zebra" }),
      makeSong({ hash: "sym", title: "!!! (Song)" }),
      makeSong({ hash: "a", title: "Apple" }),
    ];
    const sorted = sortSongs(songs, "title_asc");
    expect(sorted[0].hash).toBe("a");
    expect(sorted[1].hash).toBe("z");
    expect(sorted.slice(2).map((s) => s.hash)).toContain("num");
    expect(sorted.slice(2).map((s) => s.hash)).toContain("sym");
  });

  test("non-alphabetic artist sorts after lettered artist", () => {
    const songs = [
      makeSong({ hash: "num", artist: "3 Doors Down", title: "Kryptonite" }),
      makeSong({ hash: "z", artist: "Ziggy", title: "Star" }),
      makeSong({ hash: "a", artist: "Abba", title: "Waterloo" }),
    ];
    const sorted = sortSongs(songs, "artist_asc");
    expect(sorted[0].hash).toBe("a");
    expect(sorted[1].hash).toBe("z");
    expect(sorted[2].hash).toBe("num");
  });

  test("title_asc keeps mixed Simplified Chinese and Latin inputs in rail order", () => {
    const songs = [
      makeSong({ hash: "a", title: "Zoo" }),
      makeSong({ hash: "b", title: "北京之夜" }),
      makeSong({ hash: "c", title: "Apple" }),
    ];
    const sorted = sortSongs(songs, "title_asc");
    expect(sorted.map((song) => song.hash)).toEqual(["c", "b", "a"]);
  });

  test("title_asc keeps each mixed-script rail bucket contiguous and monotonic", () => {
    const songs = [
      makeSong({ hash: "beijing", title: "北京之夜" }),
      makeSong({ hash: "apple", title: "Apple" }),
      makeSong({ hash: "banana", title: "Banana" }),
      makeSong({ hash: "pear", title: "苹果" }),
      makeSong({ hash: "zoo", title: "Zoo" }),
    ];

    const sorted = sortSongs(songs, "title_asc");
    expect(sorted.map((song) => bucketForSortKey(song.title))).toEqual([
      "A",
      "B",
      "B",
      "P",
      "Z",
    ]);

    const index = buildAlphabetIndex(sorted, "title_asc");
    expect(index.get("A")).toBe(0);
    expect(index.get("B")).toBe(1);
    expect(index.get("P")).toBe(3);
    expect(index.get("Z")).toBe(4);
  });

  test("artist_asc keeps each mixed-script rail bucket contiguous and monotonic", () => {
    const songs = [
      makeSong({ hash: "beijing", artist: "北京之夜", title: "1" }),
      makeSong({ hash: "apple", artist: "Apple", title: "2" }),
      makeSong({ hash: "banana", artist: "Banana", title: "3" }),
      makeSong({ hash: "pear", artist: "苹果", title: "4" }),
      makeSong({ hash: "zoo", artist: "Zoo", title: "5" }),
    ];

    const sorted = sortSongs(songs, "artist_asc");
    expect(sorted.map((song) => bucketForSortKey(song.artist))).toEqual([
      "A",
      "B",
      "B",
      "P",
      "Z",
    ]);

    const index = buildAlphabetIndex(sorted, "artist_asc");
    expect(index.get("A")).toBe(0);
    expect(index.get("B")).toBe(1);
    expect(index.get("P")).toBe(3);
    expect(index.get("Z")).toBe(4);
  });

  test("non-finite imported_at collapses to 0 and sorts last when descending", () => {
    const songs = [
      makeSong({ hash: "a", imported_at: Number.NaN, title: "NaN" }),
      makeSong({ hash: "b", imported_at: 1000, title: "Real" }),
      makeSong({
        hash: "c",
        imported_at: Number.POSITIVE_INFINITY,
        title: "Inf",
      }),
    ];
    const sorted = sortSongs(songs, "recently_imported");
    expect(sorted[0].hash).toBe("b");
    // NaN and Infinity both collapse to 0; they tie on imported_at, then sort
    // by title: "Inf" < "NaN" under the collator.
    expect(sorted.map((s) => s.hash)).toEqual(["b", "c", "a"]);
  });

  test("input array is not mutated", () => {
    const songs = [
      makeSong({ hash: "b", title: "B" }),
      makeSong({ hash: "a", title: "A" }),
    ];
    const snapshot = songs.map((s) => s.hash);
    sortSongs(songs, "title_asc");
    expect(songs.map((s) => s.hash)).toEqual(snapshot);
  });

  test("compareSongs returns 0 only for fully identical songs", () => {
    const a = makeSong({ hash: "x", title: "T", artist: "A", imported_at: 5 });
    const b = makeSong({ hash: "x", title: "T", artist: "A", imported_at: 5 });
    expect(compareSongs(a, b, "recently_imported")).toBe(0);
    expect(compareSongs(a, b, "title_asc")).toBe(0);
    expect(compareSongs(a, b, "artist_asc")).toBe(0);
  });
});
