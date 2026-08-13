import { beforeEach, describe, expect, test, vi } from "vitest";

const { mockEmit, mockEmitTo, mockStartAirPlayPlainTextPagePending } =
  vi.hoisted(() => ({
    mockEmit: vi.fn(),
    mockEmitTo: vi.fn(),
    mockStartAirPlayPlainTextPagePending: vi.fn(),
  }));

vi.mock("@tauri-apps/api/event", () => ({
  emit: mockEmit,
  emitTo: mockEmitTo,
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: {
    getState: () => ({
      airPlayPlainTextPagePending: false,
      startAirPlayPlainTextPagePending: mockStartAirPlayPlainTextPagePending,
    }),
  },
}));

import { createMockBackend } from "@/lib/backend/mock-backend";
import {
  announceLocalAudienceOutputActive,
  getAirPlayPlainTextPageLockMs,
  resolvePlainTextRemoteTarget,
  stepPlainTextRemotePage,
} from "./plain-text-page-controls";

const mockStepAirPlayPlainTextPage = vi.fn();
const backend = createMockBackend({
  overrides: {
    playback: { stepAirPlayPlainTextPage: mockStepAirPlayPlainTextPage },
  },
});

describe("plain-text page controls", () => {
  beforeEach(() => {
    mockEmit.mockReset();
    mockEmitTo.mockReset();
    mockStepAirPlayPlainTextPage.mockReset();
    mockStartAirPlayPlainTextPagePending.mockReset();
    mockEmit.mockResolvedValue(undefined);
    mockEmitTo.mockResolvedValue(undefined);
    mockStepAirPlayPlainTextPage.mockResolvedValue(undefined);
  });

  test("prefers AirPlay over the local audience window when both are available", () => {
    expect(
      resolvePlainTextRemoteTarget({ active: true, phase: "playing" }, true),
    ).toBe("airplay");
    expect(
      resolvePlainTextRemoteTarget({ active: false, phase: "playing" }, true),
    ).toBe("local");
    expect(
      resolvePlainTextRemoteTarget({ active: false, phase: "idle" }, false),
    ).toBeNull();
  });

  test("routes page steps to AirPlay when AirPlay is active", async () => {
    await stepPlainTextRemotePage(
      { active: true, phase: "buffering", latencyMs: 600 },
      true,
      "next",
      backend.playback.stepAirPlayPlainTextPage,
    );

    expect(mockStepAirPlayPlainTextPage).toHaveBeenCalledWith("next");
    expect(mockStartAirPlayPlainTextPagePending).toHaveBeenCalledWith(
      "next",
      900,
    );
    expect(mockEmitTo).not.toHaveBeenCalled();
  });

  test("routes page steps to the fullscreen player when only the local audience is active", async () => {
    await stepPlainTextRemotePage(
      { active: false, phase: "idle" },
      true,
      "prev",
      backend.playback.stepAirPlayPlainTextPage,
    );

    expect(mockEmitTo).toHaveBeenCalledWith(
      "fullscreen-player",
      "openkara://local-audience-plain-text-page",
      { direction: "prev" },
    );
    expect(mockStepAirPlayPlainTextPage).not.toHaveBeenCalled();
    expect(mockStartAirPlayPlainTextPagePending).not.toHaveBeenCalled();
  });

  test("broadcasts local audience active state updates", async () => {
    await announceLocalAudienceOutputActive(true);

    expect(mockEmit).toHaveBeenCalledWith(
      "openkara://local-audience-output-state",
      { active: true },
    );
  });

  test("derives the AirPlay page lock window from measured latency", () => {
    expect(getAirPlayPlainTextPageLockMs(null)).toBe(1450);
    expect(getAirPlayPlainTextPageLockMs(400)).toBe(900);
    expect(getAirPlayPlainTextPageLockMs(2400)).toBe(2500);
  });

  test("returns null target when fullscreen player is closed and AirPlay is idle", () => {
    expect(
      resolvePlainTextRemoteTarget({ active: false, phase: "idle" }, false),
    ).toBeNull();
  });

  test("routes to AirPlay even during buffering phase", async () => {
    await stepPlainTextRemotePage(
      { active: true, phase: "buffering", latencyMs: 1200 },
      true,
      "next",
      backend.playback.stepAirPlayPlainTextPage,
    );

    expect(mockStepAirPlayPlainTextPage).toHaveBeenCalledWith("next");
    expect(mockStartAirPlayPlainTextPagePending).toHaveBeenCalledWith(
      "next",
      1450,
    );
  });
});
