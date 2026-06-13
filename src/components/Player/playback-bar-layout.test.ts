import { describe, expect, test } from "vitest";
import {
  getPlaybackBarCenterMinWidth,
  getPlaybackBarDensity,
  getPlaybackBarLayoutTokens,
  PLAYBACK_BAR_LEFT_MIN_WIDTH,
  PLAYBACK_BAR_METADATA_COLLAPSE_WIDTH,
  shouldCollapsePlaybackBarMetadata,
} from "./playback-bar-layout";

describe("getPlaybackBarDensity", () => {
  test("uses the relaxed density at and above 1120px", () => {
    expect(getPlaybackBarDensity(1120)).toBe("relaxed");
    expect(getPlaybackBarDensity(1320)).toBe("relaxed");
  });

  test("uses the compact density between 960px and 1119px", () => {
    expect(getPlaybackBarDensity(1119)).toBe("compact");
    expect(getPlaybackBarDensity(960)).toBe("compact");
  });

  test("uses the tight density below 960px", () => {
    expect(getPlaybackBarDensity(959)).toBe("tight");
    expect(getPlaybackBarDensity(760)).toBe("tight");
  });
});

describe("getPlaybackBarLayoutTokens", () => {
  test("keeps the shared left-column floor across densities", () => {
    expect(PLAYBACK_BAR_LEFT_MIN_WIDTH).toBe(112);
  });

  test("uses density-specific left max widths and master volume widths", () => {
    expect(getPlaybackBarLayoutTokens("relaxed")).toMatchObject({
      leftMaxWidth: 268,
      masterVolumeWidth: 72,
    });
    expect(getPlaybackBarLayoutTokens("compact")).toMatchObject({
      leftMaxWidth: 218,
      masterVolumeWidth: 52,
    });
    expect(getPlaybackBarLayoutTokens("tight")).toMatchObject({
      leftMaxWidth: 160,
      masterVolumeWidth: 40,
    });
  });

  test("uses the tighter seek-bar safety dimensions", () => {
    const centerMinWidth = getPlaybackBarCenterMinWidth("tight");
    // seek (180) + cluster (120) + zoneGap (10)
    expect(centerMinWidth).toBe(180 + 120 + 10);
  });

  test("keeps a non-zero center zone minimum so inner controls do not paint under the right zone", () => {
    expect(getPlaybackBarCenterMinWidth("tight")).toBeGreaterThan(180);
  });

  test("collapses now playing metadata before the transport and utility zones collide", () => {
    expect(PLAYBACK_BAR_METADATA_COLLAPSE_WIDTH).toBe(760);
    expect(shouldCollapsePlaybackBarMetadata(759)).toBe(true);
    expect(shouldCollapsePlaybackBarMetadata(760)).toBe(false);
  });
});
