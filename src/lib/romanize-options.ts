import { isLatinScript } from "lyric-romanizer/detector";
import type { Romanizer, RomanizeOptions } from "lyric-romanizer";
import type { SongLanguage } from "@/components/Library/song-list-item-menu";

/**
 * Maps each user-selectable {@link SongLanguage} to the romanization script it
 * pins in `lyric-romanizer`.
 *
 * Pinning matters because `romanizeLines` detects one dominant script for a
 * whole array. A kanji-only line in a Japanese song carries no kana, shares
 * the CJK Unicode block with Chinese, and is otherwise romanized as Mandarin
 * pinyin (恋愛 → "liàn ài" instead of "ren'ai"). Pinning the script the user
 * chose routes every line to the right engine.
 *
 * `Record<SongLanguage, …>` is total by construction: adding a `SongLanguage`
 * member without a script here is a compile error, so the map cannot drift
 * from the union. Every member has a real `ScriptType` counterpart, so no
 * `as`-cast or `@ts-expect-error` is needed.
 */
export const OPTIONS_BY_LANGUAGE: Record<SongLanguage, RomanizeOptions> = {
  mandarin: { script: "chinese", dialect: "mandarin" },
  cantonese: { script: "chinese", dialect: "cantonese" },
  japanese: { script: "japanese" },
  korean: { script: "korean" },
  cyrillic: { script: "cyrillic" },
  thai: { script: "thai" },
  devanagari: { script: "devanagari" },
  gujarati: { script: "gujarati" },
  gurmukhi: { script: "gurmukhi" },
  telugu: { script: "telugu" },
  kannada: { script: "kannada" },
  odia: { script: "odia" },
  tamil: { script: "tamil" },
};

/**
 * Romanize lyric lines with an already-created {@link Romanizer}.
 *
 * Shared by the main-thread fallback (`lyrics-romanizer.ts`) and the Web
 * Worker (`romanize.worker.ts`) so the script-pinning logic lives in exactly
 * one place. Callers still own the lazy engine load and the top-level Latin
 * short-circuit (so a pure-Latin array never loads an engine).
 */
export async function romanizeLinesWith(
  romanizer: Romanizer,
  lines: readonly string[],
  language?: SongLanguage | null,
): Promise<string[]> {
  // Unknown language: do NOT pin a script. Pass the whole array in one call so
  // the library detects the dominant script once across every line — any kana
  // anywhere is definitive proof of Japanese, whereas a per-line loop would
  // misdetect a kanji-only line as Chinese. This is the documented
  // "Mixed-script arrays" contract (see the library README) and ADR-0002.
  // Pure-Latin lines are returned unchanged by the library, so no per-line
  // pre-filter is needed on this path.
  if (language === null || language === undefined) {
    try {
      const r = await romanizer.romanizeLines(lines);
      return lines.map((line, index) => r.lines[index] ?? line);
    } catch {
      return [...lines];
    }
  }

  // Pinned language: romanize one line at a time. A single failed line falls
  // back to itself instead of failing the whole array, and the caller's
  // per-request stale-response handling is preserved. The pinned script routes
  // kanji-only lines to the right engine; the Latin pre-filter skips an
  // English chorus line so it is never fed to the pinned engine.
  const options = OPTIONS_BY_LANGUAGE[language];
  return Promise.all(
    lines.map(async (line) => {
      if (isLatinScript([line])) return line;
      try {
        const r = await romanizer.romanizeLines([line], options);
        return r.lines[0] ?? line;
      } catch {
        return line;
      }
    }),
  );
}
