import type { WordToken } from "@/types/ipc";

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
  if (parts.length !== words.length) {
    return null;
  }
  return parts;
}
