import type { Song } from "@/types/ipc";

export function songDisplayTitle(song: Song | undefined): string {
  if (!song) {
    return "";
  }
  return song.title ?? song.file_path?.split(/[\\/]/).pop() ?? song.hash;
}
