export {
  createPlaybackSession,
  isVideoSourceQueueId,
  shouldEnqueueInsteadOfReplacingCurrentSong,
  shouldLoadSeparatedStems,
  selectCurrentPositionMs,
  selectSyncDisplayPositionMs,
  shouldAnchorPlayingSinceMs,
  type PlaybackSession,
  type PlaybackSessionDeps,
  type PlaybackTransport,
  type PlaybackQueueOps,
  type VideoPlaybackTransport,
  type PositionClockState,
} from "./session";

export {
  reduceAuthoritativeSnapshot,
  reducePositionEvent,
  isStaleTransportSnapshot,
  resolvePlayingSinceMs,
} from "./position-clock";

export {
  projectAudienceState,
  buildAirPlayAudienceState,
  AIRPLAY_AUDIENCE_VIEWPORT,
  type AudienceProjectorInput,
} from "./audience-projector";
