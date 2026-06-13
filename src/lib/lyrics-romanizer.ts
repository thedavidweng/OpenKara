import { isLatinScript } from "lyric-romanizer/detector";
import type { Romanizer, RomanizeOptions } from "lyric-romanizer";
import type { SongLanguage } from "@/components/Library/song-list-item-menu";

/// Item 7: Request ID counter for matching worker responses to requests.
/// Stale responses (with a lower requestId) are discarded when the song or
/// lyric revision changes.
let nextRequestId = 0;

let romanizerPromise: Promise<Romanizer> | null = null;

async function getRomanizer() {
  romanizerPromise ??= import("lyric-romanizer").then(({ createRomanizer }) =>
    createRomanizer({ japaneseDictPath: "/dict/" }),
  );
  return romanizerPromise;
}

function buildOptions(
  language: SongLanguage | null,
): RomanizeOptions | undefined {
  if (language === "cantonese") {
    return { script: "chinese", dialect: "cantonese" };
  }
  return undefined;
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
  const options = buildOptions(language ?? null);
  const result = await Promise.all(
    lines.map(async (line) => {
      if (isLatinScript([line])) return line;
      try {
        const r = await romanizer.romanizeLines([line], options);
        return r.lines[0] ?? line;
      } catch {
        return line;
      }
    }),
  );
  return result;
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

/// Item 7: Romanize lyrics lines with request-id matching.
/// Uses a Web Worker when available, falls back to main-thread processing.
/// Returns the result and a requestId so callers can discard stale responses.
export async function romanizeLyricsLines(
  lines: readonly string[],
  language?: SongLanguage | null,
): Promise<{ result: string[]; requestId: number }> {
  if (isLatinScript(lines)) {
    return { result: [...lines], requestId: -1 };
  }

  const requestId = ++nextRequestId;
  const w = getWorker();

  // Fallback for environments without Worker support (e.g., tests).
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
