import { useEffect } from "react";
import { usePlayerStore } from "@/stores/player-store";

export function useKeyboardShortcuts(): void {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.tagName === "SELECT" ||
        target.isContentEditable
      ) {
        return;
      }

      const { snapshot, playSong, pause, seek, setVolume, positionMs } =
        usePlayerStore.getState();

      switch (e.code) {
        case "Space": {
          e.preventDefault();
          if (snapshot?.is_playing) {
            pause();
          } else if (snapshot?.song_id) {
            playSong(snapshot.song_id);
          }
          break;
        }
        case "ArrowLeft": {
          e.preventDefault();
          seek(positionMs - 5000);
          break;
        }
        case "ArrowRight": {
          e.preventDefault();
          seek(positionMs + 5000);
          break;
        }
        case "ArrowUp": {
          e.preventDefault();
          const vol = snapshot?.volume ?? 1;
          setVolume(Math.min(1, vol + 0.05));
          break;
        }
        case "ArrowDown": {
          e.preventDefault();
          const vol2 = snapshot?.volume ?? 1;
          setVolume(Math.max(0, vol2 - 0.05));
          break;
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);
}
