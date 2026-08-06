import { useRef, useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { usePlayerStore, selectCurrentPositionMs } from "@/stores/player-store";
import { formatDuration } from "@/lib/format";
import { getWaveform } from "@/lib/tauri/playback";
import {
  bucketsForRailWidth,
  getWaveformCache,
  setWaveformCache,
} from "@/lib/waveform-cache";
import type { WaveformData } from "@/types/ipc";
import {
  PLAYBACK_BAR_SEEK_MIN_WIDTH_CLASS,
  PLAYBACK_BAR_SEEK_RAIL_MIN_WIDTH_CLASS,
  PLAYBACK_BAR_TIME_LABEL_WIDTH_CLASS,
  type PlaybackBarDensity,
} from "./playback-bar-layout";

interface SeekBarProps {
  density?: PlaybackBarDensity;
}

const KEYBOARD_SEEK_STEP_MS = 5_000;
const KEYBOARD_SEEK_PAGE_STEP_MS = 30_000;

function clampPosition(positionMs: number, durationMs: number): number {
  return Math.max(0, Math.min(durationMs, positionMs));
}

export function SeekBar({ density = "relaxed" }: SeekBarProps = {}) {
  const { t } = useTranslation();
  const snapshot = usePlayerStore((s) => s.snapshot);
  const positionMs = usePlayerStore((s) => s.positionMs);
  const playingSinceMs = usePlayerStore((s) => s.playingSinceMs);
  const seek = usePlayerStore((s) => s.seek);

  const [displayPositionMs, setDisplayPositionMs] = useState(positionMs);

  useEffect(() => {
    let rafId: number;
    const tick = () => {
      const current = selectCurrentPositionMs({
        snapshot,
        positionMs,
        playingSinceMs,
      });
      setDisplayPositionMs(current);
      rafId = requestAnimationFrame(tick);
    };
    rafId = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafId);
  }, [snapshot, positionMs, playingSinceMs]);

  const durationMs = snapshot?.duration_ms ?? 0;
  const progressPercent =
    durationMs > 0 ? (displayPositionMs / durationMs) * 100 : 0;

  const barRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [dragPercent, setDragPercent] = useState(0);

  const songId = snapshot?.song_id ?? null;
  const waveformRef = useRef<WaveformData | null>(null);
  const [waveformVersion, setWaveformVersion] = useState(0);
  const [railWidth, setRailWidth] = useState(0);
  const [dpr, setDpr] = useState(() => window.devicePixelRatio || 1);
  const requestGenerationRef = useRef(0);
  const effectiveBuckets = bucketsForRailWidth(railWidth, dpr);

  useEffect(() => {
    const rail = barRef.current;
    if (!rail) return;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const measure = () => {
      const w = rail.getBoundingClientRect().width;
      setRailWidth(w);
      setWaveformVersion((v) => v + 1);
    };
    measure();
    const onResize = () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        measure();
      }, 150);
    };
    const observer = new ResizeObserver(onResize);
    observer.observe(rail);
    return () => {
      if (timer) clearTimeout(timer);
      observer.disconnect();
    };
  }, []);

  // Some platforms update devicePixelRatio on resize before media-query fires.
  useEffect(() => {
    const onResize = () => {
      const next = window.devicePixelRatio || 1;
      setDpr((prev) => (prev === next ? prev : next));
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  useEffect(() => {
    const mql = window.matchMedia(`(resolution: ${dpr}dppx)`);
    const onChange = () => {
      const next = window.devicePixelRatio || 1;
      setDpr((prev) => (prev === next ? prev : next));
    };
    mql.addEventListener("change", onChange);
    return () => mql.removeEventListener("change", onChange);
  }, [dpr]);

  useEffect(() => {
    waveformRef.current = null;
    setWaveformVersion((v) => v + 1);

    if (!songId) {
      return;
    }

    if (railWidth === 0) {
      return;
    }

    const cached = getWaveformCache(songId, effectiveBuckets);
    if (cached) {
      waveformRef.current = { peaks: cached.peaks, buckets: cached.buckets };
      setWaveformVersion((v) => v + 1);
      return;
    }

    const generation = ++requestGenerationRef.current;
    void getWaveform(songId, effectiveBuckets)
      .then((data) => {
        if (generation !== requestGenerationRef.current) return;
        waveformRef.current = data;
        setWaveformCache(songId, effectiveBuckets, data.peaks);
        setWaveformVersion((v) => v + 1);
      })
      .catch(() => {
        if (generation !== requestGenerationRef.current) return;
        waveformRef.current = null;
        setWaveformVersion((v) => v + 1);
      });

    return () => {
      requestGenerationRef.current += 1;
    };
  }, [songId, effectiveBuckets, railWidth]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const rail = barRef.current;
    if (!canvas || !rail) return;

    const waveform = waveformRef.current;
    const peaks = waveform?.peaks ?? [];
    const rect = rail.getBoundingClientRect();
    const cssWidth = rect.width;
    const cssHeight = rect.height;
    const physicalWidth = Math.round(cssWidth * dpr);
    const physicalHeight = Math.round(cssHeight * dpr);

    if (canvas.width !== physicalWidth || canvas.height !== physicalHeight) {
      canvas.width = physicalWidth;
      canvas.height = physicalHeight;
    }

    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, cssWidth, cssHeight);

    if (peaks.length === 0) {
      return;
    }

    const barWidth = cssWidth / peaks.length;
    const midY = cssHeight / 2;
    const maxBarHeight = cssHeight / 2;

    for (let i = 0; i < peaks.length; i++) {
      const peak = peaks[i];
      const barHeight = Math.max(1, peak * maxBarHeight);
      const x = i * barWidth;
      ctx.fillStyle = "rgba(255, 255, 255, 0.25)";
      ctx.fillRect(
        x,
        midY - barHeight,
        Math.max(1, barWidth - 0.5),
        barHeight * 2,
      );
    }
  }, [waveformVersion, dpr]);

  const getPercentFromEvent = useCallback((clientX: number) => {
    if (!barRef.current) return 0;
    const rect = barRef.current.getBoundingClientRect();
    return Math.max(
      0,
      Math.min(100, ((clientX - rect.left) / rect.width) * 100),
    );
  }, []);

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      setIsDragging(true);
      setDragPercent(getPercentFromEvent(e.clientX));
    },
    [getPercentFromEvent],
  );

  useEffect(() => {
    if (!isDragging) return;

    const handleMouseMove = (e: MouseEvent) => {
      setDragPercent(getPercentFromEvent(e.clientX));
    };

    const handleMouseUp = (e: MouseEvent) => {
      const percent = getPercentFromEvent(e.clientX);
      const targetMs = (percent / 100) * durationMs;
      void seek(targetMs);
      setIsDragging(false);
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [isDragging, durationMs, seek, getPercentFromEvent]);

  const displayPercent = isDragging ? dragPercent : progressPercent;
  const displayMs = isDragging
    ? (dragPercent / 100) * durationMs
    : displayPositionMs;

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      if (durationMs <= 0) {
        return;
      }

      let nextPositionMs: number | null = null;
      switch (event.key) {
        case "ArrowLeft":
        case "ArrowDown":
          nextPositionMs = displayPositionMs - KEYBOARD_SEEK_STEP_MS;
          break;
        case "ArrowRight":
        case "ArrowUp":
          nextPositionMs = displayPositionMs + KEYBOARD_SEEK_STEP_MS;
          break;
        case "PageDown":
          nextPositionMs = displayPositionMs - KEYBOARD_SEEK_PAGE_STEP_MS;
          break;
        case "PageUp":
          nextPositionMs = displayPositionMs + KEYBOARD_SEEK_PAGE_STEP_MS;
          break;
        case "Home":
          nextPositionMs = 0;
          break;
        case "End":
          nextPositionMs = durationMs;
          break;
        default:
          return;
      }

      event.preventDefault();
      void seek(clampPosition(nextPositionMs, durationMs));
    },
    [displayPositionMs, durationMs, seek],
  );

  return (
    <div
      className={`flex ${PLAYBACK_BAR_SEEK_MIN_WIDTH_CLASS} flex-1 items-center tabular-nums text-[11px] text-[var(--color-text-dim)] ${
        density === "relaxed" ? "gap-3" : "gap-2"
      }`}
    >
      <span
        className={`${PLAYBACK_BAR_TIME_LABEL_WIDTH_CLASS} shrink-0 whitespace-nowrap text-center`}
      >
        {formatDuration(displayMs)}
      </span>
      <div
        ref={barRef}
        id="seek-slider"
        className={`group relative h-1.5 ${PLAYBACK_BAR_SEEK_RAIL_MIN_WIDTH_CLASS} flex-1 rounded-full bg-[var(--color-border)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)]`}
        onMouseDown={handleMouseDown}
        role="slider"
        tabIndex={durationMs > 0 ? 0 : -1}
        aria-label={t("player.seek")}
        aria-valuemin={0}
        aria-valuemax={durationMs}
        aria-valuenow={Math.round(displayMs)}
        aria-valuetext={formatDuration(displayMs)}
        aria-disabled={durationMs <= 0 || undefined}
        onKeyDown={handleKeyDown}
      >
        {/* #90: Waveform canvas — rendered behind the progress fill. */}
        <canvas
          ref={canvasRef}
          className="pointer-events-none absolute inset-0 h-full w-full rounded-full"
          aria-hidden="true"
          data-waveform-canvas
        />
        <div
          className={`relative h-full rounded-full transition-colors ${
            isDragging
              ? "bg-[var(--color-control-primary)]"
              : "bg-[var(--color-text-dim)] group-hover:bg-[var(--color-control-primary)]"
          }`}
          style={{ width: `${displayPercent}%` }}
        >
          <div
            className={`absolute -right-1.5 top-1/2 h-3 w-3 -translate-y-1/2 rounded-full bg-[var(--color-control-primary)] shadow-sm transition-opacity ${
              isDragging ? "opacity-100" : "opacity-0 group-hover:opacity-100"
            }`}
          />
        </div>
      </div>
      <span
        className={`${PLAYBACK_BAR_TIME_LABEL_WIDTH_CLASS} shrink-0 whitespace-nowrap text-center`}
      >
        {formatDuration(durationMs)}
      </span>
    </div>
  );
}
