import { useEffect, useState } from "react";
import { LyricsPanel } from "@/components/Lyrics/LyricsPanel";
import { CdgCanvas } from "@/components/Cdg/CdgCanvas";
import { useCoverArtUrl } from "@/lib/cover-art";
import { getCoverArtPreview } from "@/lib/tauri/library";
import { useCdgStore } from "@/stores/cdg-store";
import { useLibraryStore } from "@/stores/library-store";
import { usePlayerStore } from "@/stores/player-store";
import { useSettingsStore } from "@/stores/settings-store";
import { songHasCdgMedia } from "@/lib/song-media";
import type { CoverArtBytes } from "@/types/ipc";

interface PlaybackStageProps {
  presentation?: "standard" | "audience";
}

export function PlaybackStage({
  presentation = "standard",
}: PlaybackStageProps) {
  const hasCdg = useCdgStore((s) => s.hasCdg);
  const songId = usePlayerStore((s) => s.snapshot?.song_id ?? null);
  const songs = useLibraryStore((s) => s.songs);
  const currentSong = songs.find((song) => song.hash === songId) ?? null;
  const currentSongHasCdg = songHasCdgMedia(currentSong);
  const coverArtBackdrop = useSettingsStore((s) => s.coverArtBackdrop);

  const [fetchedCoverArt, setFetchedCoverArt] = useState<CoverArtBytes | null>(
    null,
  );
  useEffect(() => {
    setFetchedCoverArt(null);
    if (currentSong?.cover_art != null || !currentSong?.has_cover_art) {
      return;
    }
    let cancelled = false;
    getCoverArtPreview(currentSong.hash)
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
    "preview",
  );
  const stageAmbience =
    coverArtBackdrop &&
    presentation === "standard" &&
    !hasCdg &&
    !currentSongHasCdg &&
    nativeBackdropUrl != null;
  const showCdg = hasCdg || currentSongHasCdg;

  return (
    <div
      className="relative flex h-full w-full flex-1 overflow-hidden"
      data-stage-visual-variant={stageAmbience ? "ambience" : "default"}
    >
      {showCdg ? (
        <CdgCanvas />
      ) : (
        <>
          {stageAmbience ? (
            <div className="absolute inset-0" data-native-stage-backdrop="true">
              <div
                className="absolute inset-[-6%] scale-[1.06] bg-center bg-cover"
                style={{
                  ...(nativeBackdropUrl
                    ? { backgroundImage: `url(${nativeBackdropUrl})` }
                    : {}),
                  opacity: "var(--ambience-backdrop-opacity)",
                  filter: "var(--ambience-backdrop-filter)",
                }}
              />
              <div className="absolute inset-0 bg-[var(--ambience-scrim)]" />
            </div>
          ) : null}
          <div
            key="lyrics-stage-slot"
            className="relative z-10 flex min-h-0 flex-1 overflow-hidden"
          >
            <LyricsPanel presentation={presentation} />
          </div>
        </>
      )}
    </div>
  );
}
