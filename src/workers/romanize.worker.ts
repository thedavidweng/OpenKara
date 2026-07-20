// Moves romanization off the main thread and uses request-id matching so
// stale responses are ignored when the song changes.

import { isLatinScript } from "lyric-romanizer/detector";
import type { Romanizer, RomanizeOptions } from "lyric-romanizer";
import type { SongLanguage } from "@/components/Library/song-list-item-menu";

interface RomanizeRequest {
  requestId: number;
  lines: readonly string[];
  language?: SongLanguage | null;
}

interface RomanizeResponse {
  requestId: number;
  result: string[];
}

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

async function romanizeLines(
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

self.onmessage = async (event: MessageEvent<RomanizeRequest>) => {
  const { requestId, lines, language } = event.data;
  try {
    const result = await romanizeLines(lines, language);
    const response: RomanizeResponse = { requestId, result };
    self.postMessage(response);
  } catch {
    const response: RomanizeResponse = {
      requestId,
      result: [...lines],
    };
    self.postMessage(response);
  }
};
