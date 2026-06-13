import { selectCurrentPositionMs } from "@/stores/player-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";

export function readLyricsAdjustedPlaybackMs(): number {
  const playerState = usePlayerStore.getState();
  const { offsetMs } = useLyricsStore.getState();
  const positionMs =
    playerState.airPlayOutput.active &&
    playerState.airPlayOutput.displayedPositionMs !== null
      ? playerState.airPlayOutput.displayedPositionMs
      : selectCurrentPositionMs(playerState);
  return positionMs - offsetMs;
}
