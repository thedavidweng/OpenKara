import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlignCenter, AlignLeft, Languages, LoaderCircle } from "lucide-react";
import { PeakMeter } from "./PeakMeter";
import { PlayControls } from "./PlayControls";
import { SeekBar } from "./SeekBar";
import { closeFullscreenPlayer } from "@/lib/fullscreen-player";
import {
  emitLocalAudienceRomanizeSetRequest,
  type LocalAudienceRomanizeSetRequest,
} from "@/lib/local-audience-romanize";
import { useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore, selectCurrentPositionMs } from "@/stores/player-store";

const CONTROLS_IDLE_MS = 3000;

export function FullscreenControls() {
  const { t } = useTranslation();
  const [idle, setIdle] = useState(true);
  const idleTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const containerRef = useRef<HTMLDivElement>(null);
  const hoveringControlsRef = useRef(false);

  useEffect(() => {
    const armIdleTimer = () => {
      clearTimeout(idleTimerRef.current);
      if (hoveringControlsRef.current) return;
      idleTimerRef.current = setTimeout(() => setIdle(true), CONTROLS_IDLE_MS);
    };

    const wake = () => {
      setIdle(false);
      armIdleTimer();
    };

    const handleControlsEnter = () => {
      hoveringControlsRef.current = true;
      clearTimeout(idleTimerRef.current);
      setIdle(false);
    };

    const handleControlsLeave = () => {
      hoveringControlsRef.current = false;
      armIdleTimer();
    };

    wake();
    window.addEventListener("pointermove", wake);
    window.addEventListener("pointerdown", wake);
    const controls = containerRef.current;
    controls?.addEventListener("pointerenter", handleControlsEnter);
    controls?.addEventListener("pointerleave", handleControlsLeave);

    return () => {
      clearTimeout(idleTimerRef.current);
      window.removeEventListener("pointermove", wake);
      window.removeEventListener("pointerdown", wake);
      controls?.removeEventListener("pointerenter", handleControlsEnter);
      controls?.removeEventListener("pointerleave", handleControlsLeave);
    };
  }, []);

  const showRomanized = useLyricsStore((s) => s.showRomanized);
  const isRomanizing = useLyricsStore((s) => s.isRomanizing);
  const lyricSongId = useLyricsStore((s) => s.songId);
  const hasLyrics = useLyricsStore((s) => s.lines.length > 0);
  const setRomanizedVisibility = useLyricsStore(
    (s) => s.setRomanizedVisibility,
  );
  const lyricsAlignment = useLyricsStore((s) => s.lyricsAlignment);
  const toggleLyricsAlignment = useLyricsStore((s) => s.toggleLyricsAlignment);
  const romanizeDisabled = !hasLyrics || !lyricSongId || isRomanizing;

  const handleRomanizeClick = () => {
    if (!lyricSongId || romanizeDisabled) return;
    const next = !showRomanized;
    setRomanizedVisibility(next);
    const request: LocalAudienceRomanizeSetRequest = {
      songId: lyricSongId,
      showRomanized: next,
    };
    void emitLocalAudienceRomanizeSetRequest(request).catch(() => {});
  };

  const handleAlignmentClick = () => {
    if (!lyricSongId || !hasLyrics) return;
    toggleLyricsAlignment();
  };

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        void closeFullscreenPlayer();
        return;
      }

      // Don't intercept keys when focus is inside an editable field.
      const target = event.target as HTMLElement | null;
      if (
        target?.tagName === "INPUT" ||
        target?.tagName === "TEXTAREA" ||
        target?.tagName === "SELECT" ||
        target?.isContentEditable
      ) {
        return;
      }

      const player = usePlayerStore.getState();
      const snapshot = player.snapshot;

      switch (event.code) {
        case "Space": {
          event.preventDefault();
          if (snapshot?.is_playing) {
            void player.pause();
          } else if (snapshot?.song_id) {
            void player.resume();
          }
          return;
        }
        case "ArrowLeft": {
          event.preventDefault();
          const pos = selectCurrentPositionMs(player);
          void player.seek(pos - 5000);
          return;
        }
        case "ArrowRight": {
          event.preventDefault();
          const pos = selectCurrentPositionMs(player);
          void player.seek(pos + 5000);
          return;
        }
        case "ArrowUp": {
          event.preventDefault();
          const volume = snapshot?.volume ?? 1;
          void player.setVolume(Math.min(1, volume + 0.05));
          return;
        }
        case "ArrowDown": {
          event.preventDefault();
          const volume = snapshot?.volume ?? 1;
          void player.setVolume(Math.max(0, volume - 0.05));
          return;
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, []);

  useEffect(() => {
    const previousCursor = document.body.style.cursor;
    document.body.style.cursor = idle ? "none" : "";
    return () => {
      document.body.style.cursor = previousCursor;
    };
  }, [idle]);

  return (
    <div
      ref={containerRef}
      data-idle={idle ? "true" : "false"}
      className={`absolute inset-x-0 bottom-0 z-50 bg-gradient-to-t from-black/80 to-transparent px-8 pb-6 pt-16 transition-opacity duration-300 ${
        idle ? "pointer-events-none opacity-0" : "opacity-100"
      }`}
    >
      <div className="flex items-center justify-center gap-6">
        <PlayControls />
        <div className="w-full max-w-2xl">
          <SeekBar />
        </div>
        <PeakMeter />
        <button
          type="button"
          data-testid="fullscreen-romanize-button"
          onClick={handleRomanizeClick}
          aria-label={t("lyrics.romanizeTooltip")}
          aria-pressed={showRomanized}
          disabled={romanizeDisabled}
          className={`motion-icon-button rounded-full border p-2 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50 ${
            showRomanized
              ? "border-[color-mix(in_srgb,var(--color-accent)_40%,var(--color-border-light))] bg-[color-mix(in_srgb,var(--color-accent)_18%,var(--color-sidebar))] text-[var(--color-control-primary)]"
              : "border-[var(--color-border-light)] bg-[var(--color-sidebar)] text-[var(--color-text-dim)] hover:border-[color-mix(in_srgb,var(--color-accent)_28%,var(--color-border-light))] hover:bg-[var(--color-hover)] hover:text-[var(--color-control-primary)]"
          }`}
        >
          {isRomanizing ? (
            <LoaderCircle size={14} className="animate-spin" />
          ) : (
            <Languages size={14} />
          )}
        </button>
        <button
          type="button"
          data-testid="fullscreen-alignment-button"
          onClick={handleAlignmentClick}
          aria-label={
            lyricsAlignment === "left"
              ? t("lyrics.switchToCentered")
              : t("lyrics.switchToLeftAligned")
          }
          aria-pressed={lyricsAlignment === "left"}
          disabled={!hasLyrics || !lyricSongId}
          className={`motion-icon-button rounded-full border p-2 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50 ${
            lyricsAlignment === "left"
              ? "border-[color-mix(in_srgb,var(--color-accent)_40%,var(--color-border-light))] bg-[color-mix(in_srgb,var(--color-accent)_18%,var(--color-sidebar))] text-[var(--color-control-primary)]"
              : "border-[var(--color-border-light)] bg-[var(--color-sidebar)] text-[var(--color-text-dim)] hover:border-[color-mix(in_srgb,var(--color-accent)_28%,var(--color-border-light))] hover:bg-[var(--color-hover)] hover:text-[var(--color-control-primary)]"
          }`}
        >
          {lyricsAlignment === "left" ? (
            <AlignLeft size={14} />
          ) : (
            <AlignCenter size={14} />
          )}
        </button>
      </div>
    </div>
  );
}
