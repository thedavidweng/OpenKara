import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download } from "lucide-react";
import { Tooltip } from "@/components/Overlay/Tooltip";
import { StreamingCover } from "@/components/Catalog/StreamingCover";
import { CoverArtThumbnail } from "@/components/Shared/CoverArtThumbnail";
import { usePlayerStore } from "@/stores/player-store";
import { useLibraryStore } from "@/stores/library-store";
import { sourceAllows, useCatalogStore } from "@/stores/catalog-store";
import type { PlaybackBarDensity } from "./playback-bar-layout";

interface NowPlayingInfoProps {
  density?: PlaybackBarDensity;
  hideCoverArt?: boolean;
}

export function NowPlayingInfo({
  density = "relaxed",
  hideCoverArt = false,
}: NowPlayingInfoProps = {}) {
  const { t } = useTranslation();
  const snapshot = usePlayerStore((s) => s.snapshot);
  const songs = useLibraryStore((s) => s.songs);
  const videoItems = useCatalogStore((s) => s.videoItems);
  const previewItems = useCatalogStore((s) => s.previewItems);
  const importTracks = useCatalogStore((s) => s.importTracks);
  const sources = useCatalogStore((s) => s.sources);
  const sourceStates = useCatalogStore((s) => s.sourceStates);

  if (!snapshot?.song_id) {
    return (
      <div className="flex items-center gap-3">
        <span className="truncate text-[12px] font-medium text-[var(--color-text-dim)]">
          {t("app.name")}
        </span>
      </div>
    );
  }

  const song = songs.find((s) => s.hash === snapshot.song_id);
  const video = videoItems[snapshot.song_id];
  const preview = previewItems[snapshot.song_id];
  const title =
    song?.title || video?.title || preview?.title || t("common.unknownTitle");
  const artist =
    song?.artist ||
    video?.channel ||
    preview?.artist ||
    t("common.unknownArtist");
  const hideArtist = density === "tight";
  const showCoverArt = !hideCoverArt && (song?.has_cover_art ?? false);

  return (
    <div
      key={snapshot.song_id}
      className={`flex items-center overflow-hidden animate-[song-fade-in_var(--motion-duration-slow)_var(--motion-ease-emphasized-out)] ${
        density === "relaxed"
          ? "gap-3"
          : density === "compact"
            ? "gap-2.5"
            : "gap-2"
      }`}
      data-now-playing-visual-variant="unified"
    >
      {showCoverArt && (
        <CoverArtThumbnail
          songHash={snapshot.song_id}
          coverArt={song?.cover_art}
          thumbnailPath={song?.artwork_thumb_path}
          alt={t("common.coverArtAlt", { title })}
          className="h-12 w-12 shrink-0"
        />
      )}
      {preview && !hideCoverArt && (
        <StreamingCover
          url={preview.cover_url}
          size={96}
          className="size-12 shrink-0 rounded-md"
        />
      )}
      <div className="flex min-w-0 flex-col overflow-hidden">
        <MarqueeTitle title={title} />
        {!hideArtist && (
          <span className="truncate text-[12px] text-[var(--color-text-dim)]">
            {artist}
          </span>
        )}
      </div>
      {preview &&
        density !== "tight" &&
        sourceAllows(
          sources.find((source) => source.source_id === preview.source_id),
          "import",
          sourceStates[preview.source_id]?.session,
        ) && (
          <Tooltip label={t("catalog.streaming.importToSing")}>
            <button
              type="button"
              aria-label={t("catalog.streaming.importToSing")}
              onClick={() =>
                void importTracks(
                  [preview.remote_track_id],
                  null,
                  preview.title,
                  preview.source_id,
                )
              }
              className="flex size-7 shrink-0 items-center justify-center rounded-full text-[var(--color-accent)] hover:bg-[var(--color-ghost-hover)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)]"
            >
              <Download className="size-4" aria-hidden="true" />
            </button>
          </Tooltip>
        )}
    </div>
  );
}

function MarqueeTitle({ title }: { title: string }) {
  const containerRef = useRef<HTMLSpanElement>(null);
  const textRef = useRef<HTMLSpanElement>(null);
  const [overflowing, setOverflowing] = useState(false);
  const [marqueeOffset, setMarqueeOffset] = useState("0px");

  useEffect(() => {
    const container = containerRef.current;
    const text = textRef.current;
    if (!container || !text) return;

    const measure = () => {
      const overflows = text.scrollWidth > container.clientWidth;
      setOverflowing(overflows);
      if (overflows) {
        const offset = container.clientWidth - text.scrollWidth;
        setMarqueeOffset(`${offset}px`);
      }
    };

    measure();

    if (typeof ResizeObserver !== "undefined") {
      const observer = new ResizeObserver(measure);
      observer.observe(container);
      return () => observer.disconnect();
    }
  }, [title]);

  return (
    <span
      ref={containerRef}
      className="block overflow-hidden text-[14px] font-semibold text-[var(--color-text)]"
    >
      <span
        ref={textRef}
        className={`inline-block whitespace-nowrap ${
          overflowing ? "marquee-active" : ""
        }`}
        style={
          overflowing
            ? ({ "--marquee-offset": marqueeOffset } as React.CSSProperties)
            : undefined
        }
      >
        {title}
      </span>
    </span>
  );
}
