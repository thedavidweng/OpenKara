export function visibleRomanizedText(
  originalText: string,
  romanizedText: string | undefined,
): string | undefined {
  if (romanizedText === undefined) {
    return undefined;
  }

  const original = normalizeLyricCompareText(originalText);
  const roman = normalizeLyricCompareText(romanizedText);
  if (roman.length === 0 || original === roman) {
    return undefined;
  }

  return romanizedText;
}

function normalizeLyricCompareText(text: string): string {
  return text.trim().replace(/\s+/g, " ").toLowerCase();
}
