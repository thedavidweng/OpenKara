export type PlaybackBarDensity = "relaxed" | "compact" | "tight";

export const PLAYBACK_BAR_LEFT_MIN_WIDTH = 112;
const PLAYBACK_BAR_SEEK_MIN_WIDTH = 180;
export const PLAYBACK_BAR_SEEK_MIN_WIDTH_CLASS = "min-w-[180px]";
export const PLAYBACK_BAR_SEEK_RAIL_MIN_WIDTH_CLASS = "min-w-[120px]";
export const PLAYBACK_BAR_TIME_LABEL_WIDTH_CLASS = "w-[3.25rem]";
const PLAYBACK_BAR_CONTROL_CLUSTER_MIN_WIDTH = 120;
export const PLAYBACK_BAR_METADATA_COLLAPSE_WIDTH = 760;
const PLAYBACK_BAR_COVER_ART_COLLAPSE_WIDTH = 780;

interface PlaybackBarLayoutTokens {
  leftMaxWidth: number;
  inlineStemVolumeWidthClass: string;
  masterVolumeWidth: number;
  masterVolumeWidthClass: string;
  outerPadding: number;
  zoneGap: number;
  rightZoneGap: number;
  masterVolumeGap: number;
  barHeightClass: string;
}

/** Single layout table shared across shells (formerly the mac-only tier table). */
const PLAYBACK_BAR_LAYOUT_TOKENS: Record<
  PlaybackBarDensity,
  PlaybackBarLayoutTokens
> = {
  relaxed: {
    leftMaxWidth: 180,
    inlineStemVolumeWidthClass: "w-[88px]",
    masterVolumeWidth: 104,
    masterVolumeWidthClass: "w-[104px]",
    outerPadding: 24,
    zoneGap: 16,
    rightZoneGap: 8,
    masterVolumeGap: 8,
    barHeightClass: "h-[86px]",
  },
  compact: {
    leftMaxWidth: 152,
    inlineStemVolumeWidthClass: "w-[72px]",
    masterVolumeWidth: 80,
    masterVolumeWidthClass: "w-[80px]",
    outerPadding: 16,
    zoneGap: 12,
    rightZoneGap: 6,
    masterVolumeGap: 6,
    barHeightClass: "h-[78px]",
  },
  tight: {
    leftMaxWidth: 148,
    // Unused inline (stems hide behind the mixer trigger); popup rows keep w-16.
    inlineStemVolumeWidthClass: "w-[64px]",
    masterVolumeWidth: 64,
    masterVolumeWidthClass: "w-[64px]",
    outerPadding: 16,
    zoneGap: 10,
    rightZoneGap: 6,
    masterVolumeGap: 6,
    barHeightClass: "h-[70px]",
  },
};

export function getPlaybackBarDensity(width: number): PlaybackBarDensity {
  if (width < 960) {
    return "tight";
  }

  if (width < 1120) {
    return "compact";
  }

  return "relaxed";
}

export function getPlaybackBarLayoutTokens(
  density: PlaybackBarDensity,
): PlaybackBarLayoutTokens {
  return PLAYBACK_BAR_LAYOUT_TOKENS[density];
}

export function getPlaybackBarCenterMinWidth(
  density: PlaybackBarDensity,
): number {
  const { zoneGap } = getPlaybackBarLayoutTokens(density);

  return (
    PLAYBACK_BAR_CONTROL_CLUSTER_MIN_WIDTH +
    PLAYBACK_BAR_SEEK_MIN_WIDTH +
    zoneGap
  );
}

export function shouldCollapsePlaybackBarMetadata(width: number): boolean {
  return width < PLAYBACK_BAR_METADATA_COLLAPSE_WIDTH;
}

export function shouldHideCoverArt(width: number): boolean {
  return width < PLAYBACK_BAR_COVER_ART_COLLAPSE_WIDTH;
}
