import type { Romanizer, RomanizeOptions } from "lyric-romanizer";
import type { SongLanguage } from "@/components/Library/song-list-item-menu";

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
  try {
    const result =
      language === null || language === undefined
        ? await romanizer.romanizeLines(lines)
        : await romanizer.romanizeLines(lines, OPTIONS_BY_LANGUAGE[language]);
    return lines.map((line, index) => result.lines[index] ?? line);
  } catch {
    return [...lines];
  }
}
