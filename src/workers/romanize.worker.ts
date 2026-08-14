import type { Romanizer } from "lyric-romanizer";
import type { SongLanguage } from "@/components/Library/song-list-item-menu";
import { romanizeLinesWith } from "@/lib/romanize-options";

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

self.onmessage = async (event: MessageEvent<RomanizeRequest>) => {
  const { requestId, lines, language } = event.data;
  try {
    const romanizer = await getRomanizer();
    const result = await romanizeLinesWith(romanizer, lines, language);
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
