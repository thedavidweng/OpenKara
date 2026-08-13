import { isLatinScript } from "lyric-romanizer/detector";
import type { SongLanguage } from "@/components/Library/song-list-item-menu";

let nextRequestId = 0;
let worker: Worker | null = null;

function getWorker(): Worker | null {
  if (typeof Worker === "undefined") return null;
  if (!worker) {
    worker = new Worker(
      new URL("../workers/romanize.worker.ts", import.meta.url),
      { type: "module" },
    );
  }
  return worker;
}

export async function romanizeLyricsLines(
  lines: readonly string[],
  language?: SongLanguage | null,
): Promise<{ result: string[]; requestId: number }> {
  if (isLatinScript(lines)) {
    return { result: [...lines], requestId: -1 };
  }

  const requestId = ++nextRequestId;
  const w = getWorker();

  if (!w) {
    return { result: [...lines], requestId };
  }

  return new Promise((resolve) => {
    const handler = (event: MessageEvent) => {
      if (event.data.requestId === requestId) {
        w.removeEventListener("message", handler);
        resolve({
          result: event.data.result as string[],
          requestId,
        });
      }
    };
    w.addEventListener("message", handler);
    w.postMessage({ requestId, lines, language });
  });
}
