import type { Song } from "@/types/ipc";

/**
 * The name to show for a song. Falls back through the file name to the content
 * hash, because a song with no metadata still has to be identifiable in a list,
 * a progress label, and a notification.
 */
export function songDisplayTitle(song: Song | undefined): string {
  if (!song) {
    return "";
  }
  return song.title ?? song.file_path?.split(/[\\/]/).pop() ?? song.hash;
}
