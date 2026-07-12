import { beforeEach, describe, expect, test } from "vitest";
import {
  markLyricsSeekFlag,
  resetLyricsPlaybackTimeForTests,
  sampleLyricsTimeFrame,
  setLyricsCurrentTime,
} from "./lyrics-playback-time";

describe("lyrics playback time feed (AMLL setCurrentTime)", () => {
  beforeEach(() => {
    resetLyricsPlaybackTimeForTests();
  });

  test("samples the host clock pushed each frame", () => {
    setLyricsCurrentTime(1200);
    expect(sampleLyricsTimeFrame()).toEqual({
      positionMs: 1200,
      isSeek: false,
    });
  });

  test("isSeek latches once then clears (AMLL isSeek)", () => {
    setLyricsCurrentTime(5000, { isSeek: true });
    expect(sampleLyricsTimeFrame()).toEqual({
      positionMs: 5000,
      isSeek: true,
    });
    setLyricsCurrentTime(5016);
    expect(sampleLyricsTimeFrame()).toEqual({
      positionMs: 5016,
      isSeek: false,
    });
  });

  test("markLyricsSeekFlag arms isSeek for the next sample", () => {
    setLyricsCurrentTime(1000);
    markLyricsSeekFlag();
    setLyricsCurrentTime(8000);
    expect(sampleLyricsTimeFrame()).toEqual({
      positionMs: 8000,
      isSeek: true,
    });
  });
});
