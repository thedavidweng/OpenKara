import type { WordToken } from "@/types/ipc";

const CJK_OR_KANA = /[一-鿿぀-ゟ゠-ヿ가-힯]/u;
const KANA_OR_CHOON = /[ぁ-んァ-ンー]/u;
const SMALL_KANA = /[ぁぃぅぇぉゃゅょゎァィゥェォャュョヮ]/u;

export type RomanFillUnit = {
  text: string;
  time_ms: number;
  end_ms: number;
};

function phoneticGlyphs(text: string): string[] {
  return Array.from(text.replace(/^\s+|\s+$/g, "")).filter(
    (glyph) => CJK_OR_KANA.test(glyph) || glyph === "ー",
  );
}

function kanaMoraCount(text: string): number | null {
  const glyphs = phoneticGlyphs(text);
  if (glyphs.length === 0) {
    return 0;
  }
  if (!glyphs.every((glyph) => KANA_OR_CHOON.test(glyph))) {
    return null;
  }

  let mora = 0;
  for (const glyph of glyphs) {
    if (SMALL_KANA.test(glyph)) {
      continue;
    }
    mora += 1;
  }
  return Math.max(1, mora);
}

function chineseGlyphCount(text: string): number | null {
  const glyphs = phoneticGlyphs(text);
  if (glyphs.length === 0) {
    return 0;
  }
  if (
    glyphs.every(
      (glyph) => CJK_OR_KANA.test(glyph) && !KANA_OR_CHOON.test(glyph),
    )
  ) {
    return glyphs.length;
  }
  return null;
}

function distributeProportionally(weights: number[], total: number): number[] {
  if (weights.length === 0) {
    return [];
  }
  const sum = weights.reduce((acc, weight) => acc + weight, 0);
  if (sum <= 0 || total <= 0) {
    return weights.map(() => 0);
  }

  const raw = weights.map((weight) => (weight / sum) * total);
  const counts = raw.map((value) => Math.floor(value));
  let remain = total - counts.reduce((acc, count) => acc + count, 0);
  const order = raw
    .map((value, index) => ({ index, frac: value - Math.floor(value) }))
    .sort((left, right) => right.frac - left.frac);
  for (let step = 0; step < remain; step += 1) {
    const slot = order[step % order.length];
    if (slot) {
      counts[slot.index] += 1;
    }
  }
  return counts;
}

function assignPartsToWords(words: WordToken[], partCount: number): number[] {
  if (partCount === words.length) {
    return words.map(() => 1);
  }

  const japaneseLine = words.some((word) =>
    Array.from(word.text).some((glyph) => KANA_OR_CHOON.test(glyph)),
  );
  const known = words.map((word) => {
    const kana = kanaMoraCount(word.text);
    if (kana !== null) {
      return kana;
    }
    return japaneseLine ? null : chineseGlyphCount(word.text);
  });

  const unknownIndexes: number[] = [];
  let knownTotal = 0;
  for (let index = 0; index < known.length; index += 1) {
    const count = known[index];
    if (count === null) {
      unknownIndexes.push(index);
    } else {
      knownTotal += count;
    }
  }

  if (unknownIndexes.length === 0 && knownTotal === partCount) {
    return known.map((count) => count ?? 0);
  }

  if (
    unknownIndexes.length > 0 &&
    partCount >= knownTotal + unknownIndexes.length
  ) {
    const leftover = partCount - knownTotal;
    const extras = leftover - unknownIndexes.length;
    const durations = unknownIndexes.map((index) =>
      Math.max(1, words[index].end_ms - words[index].time_ms),
    );
    const extraCounts = distributeProportionally(durations, extras);
    const assigned = known.map((count) => count ?? 0);
    unknownIndexes.forEach((wordIndex, slot) => {
      assigned[wordIndex] = 1 + (extraCounts[slot] ?? 0);
    });
    return assigned;
  }

  return distributeProportionally(
    words.map((word) => Math.max(1, word.end_ms - word.time_ms)),
    partCount,
  );
}

function expandFillUnits(
  words: WordToken[],
  parts: string[],
  counts: number[],
): RomanFillUnit[] {
  const units: RomanFillUnit[] = [];
  let offset = 0;
  for (let index = 0; index < words.length; index += 1) {
    const count = counts[index] ?? 0;
    const slice = parts.slice(offset, offset + count);
    offset += count;
    if (slice.length === 0) {
      continue;
    }
    const word = words[index];
    const span = Math.max(1, word.end_ms - word.time_ms);
    for (let partIndex = 0; partIndex < slice.length; partIndex += 1) {
      const start = word.time_ms + (span * partIndex) / slice.length;
      const end = word.time_ms + (span * (partIndex + 1)) / slice.length;
      units.push({
        text: slice[partIndex],
        time_ms: start,
        end_ms: end,
      });
    }
  }
  return units;
}

export function resolveRomanFillUnits(
  words: WordToken[] | null,
  lineRoman?: string,
): RomanFillUnit[] | null {
  if (words === null || words.length === 0) {
    return null;
  }

  const parts = lineRoman?.trim().split(/\s+/).filter(Boolean) ?? [];
  if (parts.length > 0) {
    return expandFillUnits(
      words,
      parts,
      assignPartsToWords(words, parts.length),
    );
  }

  const supplied = words.flatMap((word) => {
    const romanParts = word.roman?.trim().split(/\s+/).filter(Boolean) ?? [];
    if (romanParts.length === 0) {
      return [];
    }
    return expandFillUnits([word], romanParts, [romanParts.length]);
  });
  return supplied.length > 0 ? supplied : null;
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
  if (parts.length === 0) {
    return null;
  }
  if (parts.length === words.length) {
    return parts;
  }

  const counts = assignPartsToWords(words, parts.length);
  if (counts.reduce((sum, count) => sum + count, 0) !== parts.length) {
    return null;
  }

  let offset = 0;
  return counts.map((count) => {
    const slice = parts.slice(offset, offset + count);
    offset += count;
    return slice.length > 0 ? slice.join(" ") : null;
  });
}
