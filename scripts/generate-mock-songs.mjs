#!/usr/bin/env node
// @devin-regenerable: source=/Users/david/Music/OpenKara/media/*.m4a output=src/mock/preview-songs.ts covers=src/mock/covers/
//
// Generates `src/mock/preview-songs.ts` and `src/mock/covers/*.jpg` from a
// local playlist of m4a files. The output TS file is self-contained (base64
// cover art + timed lyrics inlined) so it works in both the Vite-bundled
// website preview and the Playwright E2E mock script without runtime file I/O.
//
// Lyrics are fetched from lrclib.net (https://lrclib.net/api/get) using the
// embedded title/artist/album/duration tags.  When lrclib returns synced
// lyrics (LRC with `[mm:ss.xx]` timestamps), those are used directly.  When
// lrclib returns only plain lyrics or no match, the embedded m4a `lyrics` tag
// is used with pseudo-LRC timestamps distributed evenly across the song
// duration so the lyrics panel still scrolls during playback.
//
// Usage:
//   node scripts/generate-mock-songs.mjs [--media-dir <path>] [--cover-size 300]
//
// Defaults:
//   --media-dir  ~/Music/OpenKara/media
//   --cover-size 300
//
// The generated file is committed to the repo; re-run this script only when
// the playlist changes.  Cover JPEGs are written to src/mock/covers/ and are
// also committed (human-viewable + git-efficient).

import { execFileSync } from "node:child_process";
import {
  readFileSync,
  writeFileSync,
  mkdirSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, "..");

// ── CLI args ──
const args = process.argv.slice(2);
function arg(name, fallback) {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
}
const mediaDir = arg(
  "media-dir",
  join(homedir(), "Music", "OpenKara", "media"),
);
const coverSize = Number(arg("cover-size", "300"));

// ── Playlist ──
// (hash → slug) — the slugs become file names and song hashes in the mock.
const PLAYLIST = [
  {
    hash: "905fd10b4162e0359de6a9921326ce87b65644883c7a7595226144df47c0b374",
    slug: "earfquake",
  },
  {
    hash: "589f36455e597669a513a3d9aede378798d2b3237d422513336de7d2531c493d",
    slug: "all-the-love",
  },
  {
    hash: "b6ec48f5787b3d682d3268bc2a9a99fc1a57e95c961e57a4c2767debe38e3cfd",
    slug: "counting-stars",
  },
  {
    hash: "00ef71477ab24df7b79978412ab8c1f7b9d7c09e1756740938fbb6e07f7db8fc",
    slug: "feel-good-inc",
  },
  {
    hash: "0e2682143161745172bc5b72f91e66d89af6f0d6e5a710278da9f07286e7304a",
    slug: "three-empty-words",
  },
  {
    hash: "816d7f3c52addf91da3d8bf83a26e64cfbc4a625c653b2cb7ee232dcde43720c",
    slug: "see-you-again",
  },
];

// ── Helpers ──
function sh(cmd, args, opts = {}) {
  return execFileSync(cmd, args, {
    encoding: "utf8",
    maxBuffer: 20 * 1024 * 1024,
    ...opts,
  }).trim();
}

function probeMetadata(m4aPath) {
  const out = sh("ffprobe", [
    "-v",
    "error",
    "-show_entries",
    "format=duration:format_tags=title,artist,album,language,lyrics",
    "-of",
    "json",
    m4aPath,
  ]);
  const j = JSON.parse(out);
  const tags = j.format.tags || {};
  return {
    title: tags.title ?? null,
    artist: tags.artist ?? null,
    album: tags.album ?? null,
    duration_ms: Math.round(Number(j.format.duration) * 1000),
    language: tags.language ?? null,
    lyrics: tags.lyrics ?? "",
  };
}

function probeMbid(m4aPath) {
  // exiftool -s3 prints values only, in tag-order; we request a stable order.
  const tags = [
    "MusicBrainzTrackId",
    "MusicBrainzAlbumId",
    "MusicBrainzArtistId",
    "MusicBrainzReleaseGroupId",
    "MusicBrainzAlbumArtistId",
    "MusicBrainzReleaseTrackId",
  ];
  const vals = sh("exiftool", [
    "-s3",
    ...tags.flatMap((t) => ["-" + t]),
    m4aPath,
  ])
    .split("\n")
    .map((l) => l.trim());
  const [track, album, artist, releaseGroup, albumArtist, releaseTrack] = vals;
  return { track, album, artist, releaseGroup, albumArtist, releaseTrack };
}

function extractAndDownscaleCover(m4aPath, outPath) {
  const tmp = `/tmp/mock-cover-${Date.now()}.jpg`;
  execFileSync("exiftool", ["-b", "-CoverArt", m4aPath], {
    encoding: null,
    maxBuffer: 20 * 1024 * 1024,
    stdio: ["ignore", "pipe", "ignore"],
  }).pipe?.(() => {});
  // exiftool -b writes binary to stdout; redirect via > using shell
  execFileSync(
    "bash",
    ["-c", `exiftool -b -CoverArt "${m4aPath}" > "${tmp}"`],
    { stdio: "ignore" },
  );
  execFileSync(
    "sips",
    [
      "-Z",
      String(coverSize),
      "-s",
      "format",
      "jpeg",
      "-s",
      "formatOptions",
      "80",
      tmp,
      "--out",
      outPath,
    ],
    { stdio: "ignore" },
  );
}

function fileToBase64(path) {
  return readFileSync(path).toString("base64");
}

// ── Pseudo-LRC generation ──
// The embedded lyrics are plain text (no timestamps).  We split into lines and
// distribute timestamps evenly across the song so the lyrics panel scrolls
// during playback in the website preview and E2E tests.  Section markers like
// [Chorus] are kept as lines with their own timestamp.
function generatePseudoLrc(lyrics, durationMs) {
  const lines = lyrics
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
  if (lines.length === 0) return { raw_lrc: "", lines: [] };
  const startMs = 5000; // first line at 5s
  const endMs = Math.max(startMs + 1000, durationMs - 5000); // last line 5s before end
  const step = (endMs - startMs) / Math.max(1, lines.length - 1);
  const lrcLines = lines.map((text, i) => ({
    time_ms: Math.round(startMs + step * i),
    text,
    words: null,
    bg_words: null,
    section: null,
  }));
  const raw_lrc = lrcLines
    .map((l) => {
      const totalSec = l.time_ms / 1000;
      const mm = String(Math.floor(totalSec / 60)).padStart(2, "0");
      const ss = (totalSec % 60).toFixed(2).padStart(5, "0");
      return `[${mm}:${ss}]${l.text}`;
    })
    .join("\n");
  return { raw_lrc, lines: lrcLines };
}

// ── LRC parsing ──
// Parses an LRC string into LyricLine objects, mirroring the Rust
// `parse_lrc` in src-tauri/src/lyrics/parser.rs.  Handles:
//   - `[mm:ss.xx]` timestamp tags (including multiple per line)
//   - Metadata tags `[ar:]`, `[ti:]`, `[al:]`, `[offset:±ms]`
//   - Enhanced word-level tokens `<mm:ss.xx>word`
// The offset from `[offset:]` is applied to every timestamp.
function parseTimestampTag(tag) {
  // mm:ss or mm:ss.xx or mm:ss.xxx
  const m = tag.match(/^(\d+):(\d+)(?:\.(\d+))?$/);
  if (!m) return null;
  const minutes = Number(m[1]);
  const seconds = Number(m[2]);
  if (seconds >= 60) return null;
  const frac = m[3] ?? "";
  let fractionMs = 0;
  if (frac.length === 1) fractionMs = Number(frac) * 100;
  else if (frac.length === 2) fractionMs = Number(frac) * 10;
  else if (frac.length === 3) fractionMs = Number(frac);
  else if (frac.length > 3) return null;
  return minutes * 60_000 + seconds * 1_000 + fractionMs;
}

function parseWordTokens(text) {
  if (!text.includes("<")) return null;
  const tokens = [];
  let plain = "";
  let remaining = text;
  while (remaining.length > 0) {
    if (remaining.startsWith("<")) {
      const close = remaining.indexOf(">");
      if (close < 0) {
        plain += remaining;
        break;
      }
      const tag = remaining.slice(1, close);
      const after = remaining.slice(close + 1);
      const ts = parseTimestampTag(tag);
      if (ts !== null) {
        // Find the next token start or end of string
        const nextLt = after.indexOf("<");
        const wordEnd = nextLt < 0 ? after.length : nextLt;
        const wordText = after.slice(0, wordEnd);
        const nextTs =
          nextLt >= 0
            ? parseTimestampTag(
                after.slice(nextLt + 1, after.indexOf(">", nextLt)),
              )
            : null;
        tokens.push({
          time_ms: ts,
          end_ms: nextTs ?? ts + 500,
          text: wordText,
        });
        plain += wordText;
        remaining = after.slice(wordEnd);
      } else {
        plain += remaining.slice(0, close + 1);
        remaining = after;
      }
    } else {
      const nextLt = remaining.indexOf("<");
      const chunk = nextLt < 0 ? remaining : remaining.slice(0, nextLt);
      plain += chunk;
      remaining = remaining.slice(chunk.length);
    }
  }
  if (tokens.length === 0) return null;
  return { plain: plain, tokens };
}

function parseLrc(lrc) {
  let offsetMs = 0;
  const parsed = [];
  for (const rawLine of lrc.split("\n")) {
    let cursor = rawLine;
    const timestamps = [];
    while (cursor.startsWith("[")) {
      const close = cursor.indexOf("]");
      if (close < 0) break;
      const tag = cursor.slice(1, close);
      const ts = parseTimestampTag(tag);
      if (ts !== null) {
        timestamps.push(ts);
        cursor = cursor.slice(close + 1);
        continue;
      }
      // Metadata tag
      if (tag.startsWith("offset:")) {
        const v = Number(tag.slice(7).trim());
        if (Number.isFinite(v)) offsetMs = v;
      }
      timestamps.length = 0;
      break;
    }
    if (timestamps.length === 0) continue;
    const trimmed = cursor.trim();
    const wordResult = parseWordTokens(trimmed);
    const lyricText = wordResult ? wordResult.plain : trimmed;
    const words = wordResult ? wordResult.tokens : null;
    for (const ts of timestamps) {
      parsed.push({
        time_ms: Math.max(0, ts + offsetMs),
        text: lyricText,
        words,
        bg_words: null,
        section: null,
      });
    }
  }
  parsed.sort((a, b) => a.time_ms - b.time_ms);
  return { lines: parsed, offset_ms: offsetMs };
}

// ── lrclib fetch ──
// Fetches synced lyrics from https://lrclib.net/api/get using the embedded
// metadata.  Returns { raw_lrc, lines, offset_ms, source } or null if no
// synced lyrics are available.
const LRCLIB_BASE = "https://lrclib.net";
async function fetchLrclibLyrics(meta) {
  const params = new URLSearchParams();
  params.set("track_name", meta.title ?? "");
  params.set("artist_name", meta.artist ?? "");
  if (meta.album) params.set("album_name", meta.album);
  params.set("duration", String(Math.round(meta.duration_ms / 1000)));
  const url = `${LRCLIB_BASE}/api/get?${params}`;
  try {
    const res = await fetch(url, {
      headers: { "User-Agent": "OpenKara/mock-songs-generator" },
      signal: AbortSignal.timeout(10_000),
    });
    if (res.status === 404) return null;
    if (!res.ok) throw new Error(`lrclib HTTP ${res.status}`);
    const data = await res.json();
    if (data.instrumental) return { instrumental: true };
    const synced = data.syncedLyrics;
    if (typeof synced === "string" && synced.trim().length > 0) {
      const { lines, offset_ms } = parseLrc(synced);
      if (lines.length > 0) {
        return { raw_lrc: synced, lines, offset_ms, source: "lrclib" };
      }
    }
    return null;
  } catch (err) {
    console.error(`  ⚠ lrclib fetch failed: ${err.message}`);
    return null;
  }
}

// ── Language inference ──
// The m4a language tag is often the release country (XE, US, DE, JP) rather
// than the sung language.  We infer the sung language from the script of the
// lyrics/album; default to "en".
function inferLanguage(meta) {
  const text = `${meta.title} ${meta.artist} ${meta.album} ${meta.lyrics}`;
  // CJK Unified Ideographs
  if (/[\u4e00-\u9fff\u3400-\u4dbf]/.test(text)) {
    // Hiragana/Katakana → ja
    if (/[\u3040-\u30ff]/.test(text)) return "ja";
    return "zh";
  }
  return "en";
}

// ── Main ──
const coversDir = join(repoRoot, "src", "mock", "covers");
mkdirSync(coversDir, { recursive: true });

// Clean stale covers
for (const f of readdirSync(coversDir)) {
  if (f.endsWith(".jpg")) rmSync(join(coversDir, f));
}

const songs = [];
const lyricsMap = {};

for (const { hash, slug } of PLAYLIST) {
  const m4aPath = join(mediaDir, `${hash}.m4a`);
  const meta = probeMetadata(m4aPath);
  const mbid = probeMbid(m4aPath);
  const coverPath = join(coversDir, `${slug}.jpg`);
  extractAndDownscaleCover(m4aPath, coverPath);
  const coverBase64 = fileToBase64(coverPath);
  const language = inferLanguage(meta);

  // Try lrclib for synced (timed) lyrics first; fall back to the embedded
  // plain-text lyrics with pseudo-LRC timestamps if lrclib has no match.
  const lrclibResult = await fetchLrclibLyrics(meta);
  let lyricsPayload;
  let lyricsSource;
  if (lrclibResult && !lrclibResult.instrumental) {
    lyricsPayload = {
      raw_lrc: lrclibResult.raw_lrc,
      lines: lrclibResult.lines,
      offset_ms: lrclibResult.offset_ms,
      source: "lrclib",
    };
    lyricsSource = "lrclib";
  } else if (lrclibResult && lrclibResult.instrumental) {
    lyricsPayload = { raw_lrc: "", lines: [], offset_ms: 0, source: "lrclib" };
    lyricsSource = "lrclib(instrumental)";
  } else {
    const pseudo = generatePseudoLrc(meta.lyrics, meta.duration_ms);
    lyricsPayload = {
      raw_lrc: pseudo.raw_lrc,
      lines: pseudo.lines,
      offset_ms: 0,
      source: "embedded",
    };
    lyricsSource = "embedded(pseudo-lrc)";
  }

  songs.push({
    hash: slug,
    file_path: null,
    audio_source_kind: "original",
    cdg_path: null,
    media_g_container: null,
    instrumental: false,
    language,
    title: meta.title,
    artist: meta.artist,
    album: meta.album,
    duration_ms: meta.duration_ms,
    cover_art_base64: coverBase64,
    has_cover_art: true,
    imported_at: (PLAYLIST.length - songs.length) * 100000,
    original_ext: "m4a",
    mbid,
  });

  lyricsMap[slug] = lyricsPayload;

  console.error(
    `✓ ${slug}: ${meta.title} — ${meta.artist} (${meta.duration_ms}ms, ${language}, cover ${coverBase64.length}b64chars, ${lyricsPayload.lines.length} lyric lines via ${lyricsSource})`,
  );
}

// ── Generate TS ──
const ts = `// @generated by scripts/generate-mock-songs.mjs — DO NOT EDIT BY HAND.
// Source: ${mediaDir}
// Cover art: base64-encoded 300×300 JPEG, downscaled from the original
// embedded cover art.  Lyrics: fetched from lrclib.net (synced LRC with real
// timestamps) when available; otherwise the embedded m4a \`lyrics\` tag is
// used with pseudo-LRC timestamps distributed evenly across the song
// duration.  MBIDs are preserved for future Cover Art Archive / MusicBrainz
// lookups but are not required at runtime.
//
// Re-generate after changing the playlist:
//   node scripts/generate-mock-songs.mjs

import type { LyricLine, Song } from "@/types/ipc";

/**
 * MBIDs for a preview song.  Preserved from the source m4a tags for
 * provenance and future Cover Art Archive / MusicBrainz lookups.  Not part
 * of the Song IPC contract and not required at runtime.
 */
export interface PreviewSongMbid {
  track: string;
  album: string;
  artist: string;
  releaseGroup: string;
  albumArtist: string;
  releaseTrack: string;
}

/**
 * A mock/preview song with extra metadata that is not part of the Song IPC
 * contract (MBIDs, base64 cover art).  The base64 cover is decoded to a
 * Uint8Array by {@link PREVIEW_SONGS} below so the \`cover_art\` field matches
 * the Song contract.
 */
export interface PreviewSong extends Omit<Song, "cover_art"> {
  cover_art: Uint8Array;
  mbid: PreviewSongMbid;
}

/**
 * Lyrics payload for a preview song, mirroring the shape returned by the
 * Rust \`fetch_lyrics\` IPC command.
 */
export interface PreviewLyrics {
  raw_lrc: string;
  lines: LyricLine[];
  offset_ms: number;
  source: string;
}

function decodeBase64ToUint8Array(b64: string): Uint8Array {
  const bin = atob(b64);
  const arr = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) arr[i] = bin.charCodeAt(i);
  return arr;
}

interface PreviewSongInternal extends Omit<PreviewSong, "cover_art"> {
  cover_art_base64: string;
}

const RAW_SONGS: PreviewSongInternal[] = ${JSON.stringify(songs, null, 2)};

export const PREVIEW_SONGS: PreviewSong[] = RAW_SONGS.map((s) => {
  const { cover_art_base64, ...rest } = s;
  return { ...rest, cover_art: decodeBase64ToUint8Array(cover_art_base64) };
});

export const PREVIEW_LYRICS: Record<string, PreviewLyrics> = ${JSON.stringify(lyricsMap, null, 2)};

/**
 * The hash of the song that the website preview selects by default and that
 * E2E tests double-click to start playback.  Kept here so the website and
 * E2E mock agree on the "primary" fixture song.
 */
export const PRIMARY_PREVIEW_SONG_HASH = ${JSON.stringify(songs[0].hash)};
`;

const outPath = join(repoRoot, "src", "mock", "preview-songs.ts");
writeFileSync(outPath, ts);
console.error(`\nWrote ${outPath} (${ts.length} chars)`);
console.error(`Wrote ${songs.length} covers to ${coversDir}/`);
