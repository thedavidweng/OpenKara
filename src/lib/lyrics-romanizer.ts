import { isLatinScript } from "lyric-romanizer/detector";
import type { Romanizer } from "lyric-romanizer";
import type { SongLanguage } from "@/components/Library/song-list-item-menu";
import { romanizeLinesWith } from "./romanize-options";

let nextRequestId = 0;

let romanizerPromise: Promise<Romanizer> | null = null;

async function getRomanizer() {
  romanizerPromise ??= import("lyric-romanizer").then(({ createRomanizer }) =>
    createRomanizer({ japaneseDictPath: "/dict/" }),
  );
  return romanizerPromise;
}

/// Synchronous romanization fallback for non-worker environments (tests).
async function romanizeLinesDirect(
  lines: readonly string[],
  language?: SongLanguage | null,
): Promise<string[]> {
  if (isLatinScript(lines)) {
    return [...lines];
  }

  const romanizer = await getRomanizer();
  return romanizeLinesWith(romanizer, lines, language);
}

/// Item 7: Lazy-initialized Web Worker for romanization.
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
    const result = await romanizeLinesDirect(lines, language);
    return { result, requestId };
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
      // Stale responses are silently ignored.
    };
    w.addEventListener("message", handler);
    w.postMessage({ requestId, lines, language });
  });
}
