import { describe, expect, test } from "vitest";
import {
  AIRPLAY_AUDIENCE_VIEWPORT,
  buildAirPlayAudienceState,
  projectAudienceState,
} from "./audience-projector";

const messages = {
  selectSong: "Select a song to start",
  loadingLyrics: "Loading lyrics...",
  noLyrics: "No lyrics available for this track",
  addLyrics: "Add Lyrics",
};

describe("projectAudienceState", () => {
  test("returns idle when no song is loaded", () => {
    expect(
      projectAudienceState({
        playbackSnapshot: null,
        lyricsSongId: null,
        lines: [],
        offsetMs: 0,
        isLoading: false,
        lyricsFontStep: 0,
        hasCdg: false,
        currentSongHasCdg: false,
        messages,
      }),
    ).toMatchObject({
      mode: "idle",
      songId: null,
      lines: [],
      viewport: AIRPLAY_AUDIENCE_VIEWPORT,
    });
  });

  test("returns lyrics payload when the current song is lyric-driven", () => {
    const result = projectAudienceState({
      playbackSnapshot: {
        song_id: "song-1",
        transport_generation: 1,
        state: "playing",
        is_playing: true,
        position_ms: 1234,
        duration_ms: 5000,
        volume: 1,
        stem_volumes: {
          vocals: 1,
          drums: 1,
          bass: 1,
          other: 1,
        },
        has_stems: false,
        stem_mode: null,
        buffered_ms: 5000,
      },
      lyricsSongId: "song-1",
      lines: [
        {
          time_ms: 0,
          text: "Hello",
          words: null,
          bg_words: null,
          section: null,
          roman: null,
        },
      ],
      offsetMs: 50,
      isLoading: false,
      lyricsFontStep: 0,
      hasCdg: false,
      currentSongHasCdg: false,
      messages,
    });

    expect(result.mode).toBe("lyrics");
    expect(result.songId).toBe("song-1");
    expect(result.lines).toHaveLength(1);
    expect(result.offsetMs).toBe(50);
  });

  test("returns cdg mode when media has CDG", () => {
    const result = projectAudienceState({
      playbackSnapshot: {
        song_id: "song-1",
        transport_generation: 1,
        state: "playing",
        is_playing: true,
        position_ms: 0,
        duration_ms: 5000,
        volume: 1,
        stem_volumes: {
          vocals: 1,
          drums: 1,
          bass: 1,
          other: 1,
        },
        has_stems: false,
        stem_mode: null,
        buffered_ms: 5000,
      },
      lyricsSongId: "song-1",
      lines: [
        {
          time_ms: 0,
          text: "Hello",
          words: null,
          bg_words: null,
          section: null,
          roman: null,
        },
      ],
      offsetMs: 0,
      isLoading: false,
      lyricsFontStep: 0,
      hasCdg: true,
      currentSongHasCdg: false,
      messages,
    });

    expect(result.mode).toBe("cdg");
    expect(result.lines).toEqual([]);
  });

  test("clears lines when lyrics belong to a different song", () => {
    const result = projectAudienceState({
      playbackSnapshot: {
        song_id: "song-2",
        transport_generation: 1,
        state: "playing",
        is_playing: true,
        position_ms: 0,
        duration_ms: 5000,
        volume: 1,
        stem_volumes: {
          vocals: 1,
          drums: 1,
          bass: 1,
          other: 1,
        },
        has_stems: false,
        stem_mode: null,
        buffered_ms: 5000,
      },
      lyricsSongId: "song-1",
      lines: [
        {
          time_ms: 0,
          text: "Stale",
          words: null,
          bg_words: null,
          section: null,
          roman: null,
        },
      ],
      offsetMs: 0,
      isLoading: false,
      lyricsFontStep: 0,
      hasCdg: false,
      currentSongHasCdg: false,
      messages,
    });

    expect(result.mode).toBe("lyrics");
    expect(result.lines).toEqual([]);
    expect(result.isLoading).toBe(true);
  });
});

describe("buildAirPlayAudienceState", () => {
  test("delegates to projectAudienceState", () => {
    expect(
      buildAirPlayAudienceState({
        playbackSnapshot: null,
        lyricsSongId: null,
        lines: [],
        offsetMs: 0,
        isLoading: false,
        lyricsFontStep: 0,
        hasCdg: false,
        currentSongHasCdg: false,
        messages,
      }),
    ).toMatchObject({
      mode: "idle",
      songId: null,
      viewport: AIRPLAY_AUDIENCE_VIEWPORT,
    });
  });
});
