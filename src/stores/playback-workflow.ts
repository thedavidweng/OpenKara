/**
 * Compatibility shim — playback lifecycle lives in `@/playback/session`.
 * Prefer importing from `@/playback` (or `@/playback/session`) in new code.
 */
export {
  createPlaybackSession as createPlaybackWorkflow,
  shouldEnqueueInsteadOfReplacingCurrentSong,
  shouldLoadSeparatedStems,
  type PlaybackSession as PlaybackWorkflow,
  type PlaybackSessionDeps as PlaybackWorkflowDeps,
} from "@/playback/session";
