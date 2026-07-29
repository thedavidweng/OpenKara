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

export async function romanizeLinesWith(
  romanizer: Romanizer,
  lines: readonly string[],
  language?: SongLanguage | null,
): Promise<string[]> {
  if (language === null || language === undefined) {
    try {
      const r = await romanizer.romanizeLines(lines);
      return lines.map((line, index) => r.lines[index] ?? line);
    } catch {
      return [...lines];
    }
  }

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
