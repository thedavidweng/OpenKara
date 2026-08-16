import type { WordToken } from "@/types/ipc";

const CJK_OR_KANA = /[一-鿿぀-ゟ゠-ヿ가-힯]/u;

function packingUnits(text: string): number {
  const glyphs = Array.from(text.replace(/^\s+|\s+$/g, "")).filter(
    (glyph) => !/\s/u.test(glyph),
  );
  if (glyphs.length === 0) {
    return 1;
  }
  if (glyphs.every((glyph) => CJK_OR_KANA.test(glyph))) {
    return glyphs.length;
  }
  return 1;
}

function packRomansByGlyphCount(
  words: WordToken[],
  parts: string[],
): string[] | null {
  const units = words.map((word) => packingUnits(word.text));
  const total = units.reduce((sum, count) => sum + count, 0);
  if (total !== parts.length) {
    return null;
  }

  let offset = 0;
  return units.map((count) => {
    const slice = parts.slice(offset, offset + count);
    offset += count;
    return slice.join(" ");
  });
}

export function resolveWordRomans(
  words: WordToken[] | null,
  lineRoman?: string,
): Array<string | null> | null {
  if (words === null || words.length === 0) {
    return null;
  }

  const supplied = words.map((word) => word.roman?.trim() || "");
  if (supplied.some((roman) => roman.length > 0)) {
    return supplied.map((roman) => (roman.length > 0 ? roman : null));
  }

  const parts = lineRoman?.trim().split(/\s+/).filter(Boolean) ?? [];
  if (parts.length === words.length) {
    return parts;
  }
  return packRomansByGlyphCount(words, parts);
}
