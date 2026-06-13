import { useEffect, useState } from "react";
import { LyricsPanel } from "@/components/Lyrics/LyricsPanel";
import { CdgCanvas } from "@/components/Cdg/CdgCanvas";
import { useCoverArtUrl } from "@/lib/cover-art";
import { getCoverArt } from "@/lib/tauri/library";
import { useCdgStore } from "@/stores/cdg-store";
import { useLibraryStore } from "@/stores/library-store";
import { usePlayerStore } from "@/stores/player-store";
import { useSettingsStore } from "@/stores/settings-store";
import { songHasCdgMedia } from "@/lib/song-media";
import type { CoverArtBytes } from "@/types/ipc";

interface PlaybackStageProps {
  presentation?: "standard" | "audience";
  bottomInsetPx?: number;
}

export function PlaybackStage({
  presentation = "standard",
  bottomInsetPx = 0,
}: PlaybackStageProps) {
  const hasCdg = useCdgStore((s) => s.hasCdg);
  const songId = usePlayerStore((s) => s.snapshot?.song_id ?? null);
  const songs = useLibraryStore((s) => s.songs);
  const currentSong = songs.find((song) => song.hash === songId) ?? null;
  const currentSongHasCdg = songHasCdgMedia(currentSong);
  const coverArtBackdrop = useSettingsStore((s) => s.coverArtBackdrop);

  // Fetch cover art on-demand for backdrop when not included in list results.
  const [fetchedCoverArt, setFetchedCoverArt] = useState<CoverArtBytes | null>(
    null,
  );
  useEffect(() => {
    if (currentSong?.cover_art != null || !currentSong?.has_cover_art) {
      setFetchedCoverArt(null);
      return;
    }
    let cancelled = false;
    getCoverArt(currentSong.hash)
      .then((data) => {
        if (!cancelled) setFetchedCoverArt(data);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [currentSong?.hash, currentSong?.cover_art, currentSong?.has_cover_art]);

  const nativeBackdropUrl = useCoverArtUrl(
    songId ?? "native-stage-empty",
    currentSong?.cover_art ?? fetchedCoverArt ?? null,
  );
  const stageAmbience =
    coverArtBackdrop &&
    presentation === "standard" &&
    !hasCdg &&
    !currentSongHasCdg;

  return (
    <div
      className="relative flex h-full w-full flex-1 overflow-hidden"
      data-stage-visual-variant={stageAmbience ? "ambience" : "default"}
      style={
        presentation === "audience" && bottomInsetPx > 0
          ? { paddingBottom: bottomInsetPx }
          : undefined
      }
    >
      {stageAmbience ? (
        <>
          <div className="absolute inset-0" data-native-stage-backdrop="true">
            <div
              className="absolute inset-[-6%] scale-[1.06] bg-center bg-cover opacity-40 blur-2xl saturate-[0.85] brightness-[0.75]"
              style={
                nativeBackdropUrl
                  ? { backgroundImage: `url(${nativeBackdropUrl})` }
                  : undefined
              }
            />
            <div className="absolute inset-0 bg-[radial-gradient(circle_at_center,rgba(0,0,0,0.35),rgba(0,0,0,0.55)_36%,rgba(0,0,0,0.72)_100%)]" />
            <div className="absolute inset-0 bg-[linear-gradient(180deg,rgba(8,10,14,0.45),rgba(8,10,14,0.62)_58%,rgba(10,12,16,0.78))]" />
          </div>
          <div className="relative z-10 flex min-h-0 flex-1 overflow-hidden">
            <LyricsPanel presentation={presentation} />
          </div>
        </>
      ) : hasCdg || currentSongHasCdg ? (
        <CdgCanvas />
      ) : (
        <LyricsPanel presentation={presentation} />
      )}
    </div>
  );
}
