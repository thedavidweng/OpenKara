import type { SongLanguage } from "@/components/Library/song-list-item-menu";

export async function romanizeLyricsLines(
  lines: readonly string[],
  _language?: SongLanguage | null,
): Promise<{ result: string[]; requestId: number }> {
  return { result: [...lines], requestId: -1 };
}
