import { describe, expect, test } from "vitest";
import {
  ALPHABET_BUCKETS,
  type AlphabetBucket,
  bucketForSortKey,
  buildAlphabetIndex,
  resolveBucket,
} from "./alphabet-index";
import type { Song } from "@/types/ipc";

function makeSong(overrides: Partial<Song> = {}): Song {
  return {
    hash: "hash",
    file_path: "/music/a.mp3",
    audio_source_kind: "original",
    cdg_path: null,
    media_g_container: null,
    instrumental: false,
    language: null,
    title: "Title",
    artist: "Artist",
    album: null,
    duration_ms: 120000,
    cover_art: null,
    has_cover_art: false,
    artwork_thumb_path: null,
    imported_at: 0,
    original_ext: "mp3",
    ...overrides,
  };
}

describe("bucketForSortKey", () => {
  test("maps A–Z letters to their uppercase bucket", () => {
    expect(bucketForSortKey("Apple")).toBe("A");
    expect(bucketForSortKey("banana")).toBe("B");
    expect(bucketForSortKey("Zebra")).toBe("Z");
  });

  test("maps Latin diacritics via NFD mark stripping", () => {
    expect(bucketForSortKey("Élan")).toBe("E");
    expect(bucketForSortKey("über")).toBe("U");
    expect(bucketForSortKey("Ñoño")).toBe("N");
    // Æ (U+00C6) is a ligature that does not decompose under NFD, so it maps to #
    expect(bucketForSortKey("Æsir")).toBe("#");
  });

  test("maps digits and punctuation to #", () => {
    expect(bucketForSortKey("123 Song")).toBe("#");
    expect(bucketForSortKey("!Bang")).toBe("#");
    expect(bucketForSortKey(".hidden")).toBe("#");
  });

  test("maps empty, null, and whitespace to #", () => {
    expect(bucketForSortKey("")).toBe("#");
    expect(bucketForSortKey(null)).toBe("#");
    expect(bucketForSortKey("   ")).toBe("#");
    expect(bucketForSortKey("\t\n")).toBe("#");
  });

  test("maps basic Han characters via pinyin first initial", () => {
    expect(bucketForSortKey("北京之夜")).toBe("B");
    expect(bucketForSortKey("苹果")).toBe("P");
    expect(bucketForSortKey("中国")).toBe("Z");
  });

  test("maps supplementary-plane Han characters", () => {
    // 𠀀 is a CJK Ext B character; pinyin-pro should still return an initial
    // or fall back to #. We just verify it doesn't crash and returns a valid bucket.
    const bucket = bucketForSortKey("𠀀");
    expect(ALPHABET_BUCKETS).toContain(bucket);
  });

  test("normalizes to NFC before segmenting", () => {
    // É as a single composed character (NFC) vs decomposed (NFD)
    const nfc = "Élan".normalize("NFC");
    const nfd = "Élan".normalize("NFD");
    expect(bucketForSortKey(nfc)).toBe(bucketForSortKey(nfd));
  });

  test("trims leading/trailing Unicode whitespace", () => {
    expect(bucketForSortKey("  Apple  ")).toBe("A");
    expect(bucketForSortKey("\u3000北京\u3000")).toBe("B");
  });
});

describe("buildAlphabetIndex", () => {
  test("stores only the first index for each bucket in title_asc mode", () => {
    const songs = [
      makeSong({ hash: "a", title: "Apple", artist: "Z" }),
      makeSong({ hash: "b", title: "Avocado", artist: "Y" }),
      makeSong({ hash: "c", title: "Banana", artist: "X" }),
    ];

    const index = buildAlphabetIndex(songs, "title_asc");

    expect(index.get("A")).toBe(0);
    expect(index.get("B")).toBe(2);
    expect(index.size).toBe(2);
  });

  test("uses artist in artist_asc mode", () => {
    const songs = [
      makeSong({ hash: "a", title: "Z", artist: "Beatles" }),
      makeSong({ hash: "b", title: "Y", artist: "Abba" }),
    ];

    const index = buildAlphabetIndex(songs, "artist_asc");

    expect(index.get("A")).toBe(1);
    expect(index.get("B")).toBe(0);
  });

  test("does not mutate the input array", () => {
    const songs = [
      makeSong({ hash: "a", title: "Apple" }),
      makeSong({ hash: "b", title: "Banana" }),
    ];
    const snapshot = [...songs];

    buildAlphabetIndex(songs, "title_asc");

    expect(songs).toEqual(snapshot);
  });

  test("returns an empty map for an empty array", () => {
    const index = buildAlphabetIndex([], "title_asc");
    expect(index.size).toBe(0);
  });

  test("maps Han titles via pinyin initial", () => {
    const songs = [
      makeSong({ hash: "a", title: "北京之夜" }),
      makeSong({ hash: "b", title: "苹果" }),
    ];

    const index = buildAlphabetIndex(songs, "title_asc");

    expect(index.get("B")).toBe(0);
    expect(index.get("P")).toBe(1);
  });
});

describe("resolveBucket", () => {
  test("returns exact index when bucket is mapped", () => {
    const index = new Map<AlphabetBucket, number>([
      ["A", 0],
      ["B", 5],
      ["Z", 25],
    ]);
    const result = resolveBucket(index, "B");
    expect(result).toEqual({ index: 5, bucket: "B" });
  });

  test("resolves to the nearest mapped bucket by distance", () => {
    const index = new Map<AlphabetBucket, number>([
      ["A", 0],
      ["Z", 25],
    ]);
    const result = resolveBucket(index, "C");
    expect(result).toEqual({ index: 0, bucket: "A" });
  });

  test("breaks ties toward the following bucket", () => {
    const index = new Map<AlphabetBucket, number>([
      ["A", 0],
      ["C", 2],
    ]);
    const result = resolveBucket(index, "B");
    expect(result).toEqual({ index: 2, bucket: "C" });
  });

  test("does not jump to # when a nearer letter bucket exists", () => {
    const index = new Map<AlphabetBucket, number>([
      ["W", 22],
      ["#", 26],
    ]);
    const result = resolveBucket(index, "X");
    expect(result).toEqual({ index: 22, bucket: "W" });
  });

  test("falls backward when no following bucket exists", () => {
    const index = new Map<AlphabetBucket, number>([
      ["A", 0],
      ["M", 13],
    ]);
    const result = resolveBucket(index, "Z");
    expect(result).toEqual({ index: 13, bucket: "M" });
  });

  test("returns null when the map is empty", () => {
    const index = new Map<AlphabetBucket, number>();
    expect(resolveBucket(index, "A")).toBeNull();
  });

  test("returns exact for # bucket", () => {
    const index = new Map<AlphabetBucket, number>([["#", 10]]);
    const result = resolveBucket(index, "#");
    expect(result).toEqual({ index: 10, bucket: "#" });
  });

  test("falls backward from A when A is not mapped", () => {
    const index = new Map<AlphabetBucket, number>([["M", 13]]);
    const result = resolveBucket(index, "A");
    expect(result).toEqual({ index: 13, bucket: "M" });
  });

  test("falls forward from # when # is not mapped", () => {
    const index = new Map<AlphabetBucket, number>([["A", 0]]);
    const result = resolveBucket(index, "#");
    expect(result).toEqual({ index: 0, bucket: "A" });
  });
});
