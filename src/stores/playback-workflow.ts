/**
 * Compatibility shim — playback lifecycle lives in `@/playback`.
 * Prefer importing from `@/playback` in new code.
 */
export {
  createPlaybackSession as createPlaybackWorkflow,
  shouldEnqueueInsteadOfReplacingCurrentSong,
  shouldLoadSeparatedStems,
  type PlaybackSession as PlaybackWorkflow,
  type PlaybackSessionDeps as PlaybackWorkflowDeps,
} from "@/playback";
