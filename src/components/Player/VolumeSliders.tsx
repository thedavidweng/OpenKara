import { Mic2, Music } from "lucide-react";
import { usePlayerStore } from "@/stores/player-store";
import { useLibraryStore } from "@/stores/library-store";

export function VolumeSliders() {
  const snapshot = usePlayerStore((s) => s.snapshot);
  const setVolume = usePlayerStore((s) => s.setVolume);
  const setMode = usePlayerStore((s) => s.setMode);
  const separationStatuses = useLibraryStore((s) => s.separationStatuses);

  const volume = snapshot?.volume ?? 1;
  const mode = snapshot?.mode ?? "original";
  const songId = snapshot?.song_id;
  const isSeparated =
    songId != null && separationStatuses[songId]?.state === "completed";

  // Vocal slider: 100 in original mode, 0 in karaoke mode
  const vocalValue = mode === "karaoke" ? 0 : 100;

  const handleVocalChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = Number(e.target.value);
    if (value === 0 && isSeparated) {
      setMode("karaoke");
    } else if (value > 0 && mode === "karaoke") {
      setMode("original");
    }
  };

  const handleInstChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setVolume(Number(e.target.value) / 100);
  };

  return (
    <div className="flex items-center gap-5">
      <div className="flex items-center gap-2" title="Vocals">
        <Mic2
          size={14}
          className={
            vocalValue > 0
              ? "text-[#EBEBF5]"
              : "text-[var(--color-text-dimmer)]"
          }
        />
        <input
          type="range"
          min="0"
          max="100"
          value={vocalValue}
          onChange={handleVocalChange}
          className="native-slider w-16"
          disabled={!isSeparated}
        />
      </div>
      <div className="flex items-center gap-2" title="Instrumentals">
        <Music
          size={14}
          className={
            volume > 0 ? "text-[#EBEBF5]" : "text-[var(--color-text-dimmer)]"
          }
        />
        <input
          type="range"
          min="0"
          max="100"
          value={Math.round(volume * 100)}
          onChange={handleInstChange}
          className="native-slider w-16"
        />
      </div>
    </div>
  );
}
