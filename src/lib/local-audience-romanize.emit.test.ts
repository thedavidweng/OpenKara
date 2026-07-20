import { beforeEach, describe, expect, test, vi } from "vitest";

const { mockEmit, mockEmitTo } = vi.hoisted(() => ({
  mockEmit: vi.fn(),
  mockEmitTo: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  emit: mockEmit,
  emitTo: mockEmitTo,
}));

import {
  LOCAL_AUDIENCE_ROMANIZE_SET_EVENT,
  LOCAL_AUDIENCE_ROMANIZE_STATE_EVENT,
  LOCAL_AUDIENCE_ROMANIZE_SYNC_REQUEST_EVENT,
  FULLSCREEN_PLAYER_WINDOW_LABEL,
  MAIN_WINDOW_LABEL,
  emitLocalAudienceRomanizeSetRequest,
  emitLocalAudienceRomanizeState,
  emitLocalAudienceRomanizeSyncRequest,
} from "./local-audience-romanize";

describe("local-audience-romanize emit helpers", () => {
  beforeEach(() => {
    mockEmit.mockReset();
    mockEmitTo.mockReset();
    mockEmit.mockResolvedValue(undefined);
    mockEmitTo.mockResolvedValue(undefined);
  });

  test("emitLocalAudienceRomanizeState targets the fullscreen-player window", async () => {
    const state = {
      revision: 7,
      songId: "song-1",
      lyricsIdentity: "id",
      showRomanized: true,
      isRomanizing: false,
      romanizedLines: ["ni hao"],
    };
    await emitLocalAudienceRomanizeState(state);

    expect(mockEmitTo).toHaveBeenCalledWith(
      FULLSCREEN_PLAYER_WINDOW_LABEL,
      LOCAL_AUDIENCE_ROMANIZE_STATE_EVENT,
      state,
    );
    expect(mockEmit).not.toHaveBeenCalled();
  });

  test("emitLocalAudienceRomanizeSetRequest targets the main window with the explicit desired boolean", async () => {
    await emitLocalAudienceRomanizeSetRequest({
      songId: "song-1",
      showRomanized: true,
    });

    expect(mockEmitTo).toHaveBeenCalledWith(
      MAIN_WINDOW_LABEL,
      LOCAL_AUDIENCE_ROMANIZE_SET_EVENT,
      { songId: "song-1", showRomanized: true },
    );
  });

  test("emitLocalAudienceRomanizeSyncRequest broadcasts via emit", async () => {
    await emitLocalAudienceRomanizeSyncRequest();

    expect(mockEmit).toHaveBeenCalledWith(
      LOCAL_AUDIENCE_ROMANIZE_SYNC_REQUEST_EVENT,
      {},
    );
    expect(mockEmitTo).not.toHaveBeenCalled();
  });
});
