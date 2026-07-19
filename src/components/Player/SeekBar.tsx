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

export function SeekBar({ density = "relaxed" }: SeekBarProps = {}) {
  const { t } = useTranslation();
  const snapshot = usePlayerStore((s) => s.snapshot);
  const positionMs = usePlayerStore((s) => s.positionMs);
  const playingSinceMs = usePlayerStore((s) => s.playingSinceMs);
  const seek = usePlayerStore((s) => s.seek);

  // Use a rAF loop to smoothly extrapolate the current position between
  // IPC position events. This prevents the progress bar from jumping
  // per-event and provides buttery-smooth animation.
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

  // #90: Waveform peaks keyed by (song, effective buckets). Bucket count is
  // derived from rail CSS width and device pixel ratio
  // (clamp(round(cssWidth * dpr / 3), 24, 1000)). Module LRU (96) serves
  // hits synchronously; async fetches use a generation so late responses
  // for a previous song/bucket never paint.
  const songId = snapshot?.song_id ?? null;
  const waveformRef = useRef<WaveformData | null>(null);
  const [waveformVersion, setWaveformVersion] = useState(0);
  const [railWidth, setRailWidth] = useState(0);
  const [dpr, setDpr] = useState(() => window.devicePixelRatio || 1);
  const requestGenerationRef = useRef(0);
  const effectiveBuckets = bucketsForRailWidth(railWidth, dpr);

  // Observe rail CSS width via ResizeObserver. DPR is tracked separately
  // (window resize + resolution media-query change) because a display
  // migration can change DPR without altering CSS geometry.
  //
  // The initial measurement is synchronous so the first fetch uses the
  // real rail width instead of the placeholder 200-bucket fallback.
  // Subsequent resize events are debounced so a continuous window/rail
  // drag does not fire a distinct bucket count on every layout tick —
  // each distinct bucket count is a cache miss that triggers a full
  // backend audio decode, so debouncing prevents many concurrent
  // full-file decodes during a resize.
  useEffect(() => {
    const rail = barRef.current;
    if (!rail) return;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const measure = () => {
      const w = rail.getBoundingClientRect().width;
      setRailWidth(w);
      // Always bump redraw so DPR / geometry changes repaint even when the
      // bucket count is unchanged.
      setWaveformVersion((v) => v + 1);
    };
    // Synchronous initial measurement — establishes the real bucket count
    // before the first fetch effect runs.
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

  // Track DPR via window resize. Some platforms update devicePixelRatio
  // synchronously on resize before any media-query fires.
  useEffect(() => {
    const onResize = () => {
      const next = window.devicePixelRatio || 1;
      setDpr((prev) => (prev === next ? prev : next));
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  // Track DPR via a resolution media-query tuned to the current DPR. When
  // DPR changes the query is re-registered against the new value so a
  // subsequent migration (e.g. 2x -> 1x) is still observed.
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
    // Drop previous peaks immediately so a resize-driven redraw during the
    // fetch window never paints the wrong song/bucket shape.
    waveformRef.current = null;
    setWaveformVersion((v) => v + 1);

    if (!songId) {
      return;
    }

    // Skip the fetch until the rail has been measured. On the first render
    // railWidth is 0, so effectiveBuckets falls back to the placeholder 200.
    // Fetching at that count would decode the whole audio file for a
    // (song_hash, 200) cache key that is immediately superseded once the
    // ResizeObserver reports the real width — a redundant full decode on
    // the first play of every song.
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
        // Backend may be unavailable during startup — keep placeholder empty.
        if (generation !== requestGenerationRef.current) return;
        waveformRef.current = null;
        setWaveformVersion((v) => v + 1);
      });

    return () => {
      // Invalidate in-flight work for this song/bucket pairing.
      requestGenerationRef.current += 1;
    };
  }, [songId, effectiveBuckets, railWidth]);

  // Render the waveform on a DPR-aware canvas behind the seek rail.
  useEffect(() => {
    const canvas = canvasRef.current;
    const rail = barRef.current;
    if (!canvas || !rail) return;

    const waveform = waveformRef.current;
    const peaks = waveform?.peaks ?? [];
    // Use the tracked `dpr` state (not `window.devicePixelRatio` directly) so
    // a DPR change that leaves `effectiveBuckets` unchanged still re-runs this
    // effect via the `dpr` dependency and repaints at the new physical pixel
    // dimensions. Reading the global directly would silently keep the stale
    // backing store because the effect would not re-trigger.
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

    // Render each peak as a vertical bar. The bar width is derived from
    // the bucket count and the rail width so the waveform always spans
    // the full rail regardless of bucket count.
    const barWidth = cssWidth / peaks.length;
    const midY = cssHeight / 2;
    const maxBarHeight = cssHeight / 2;

    for (let i = 0; i < peaks.length; i++) {
      const peak = peaks[i];
      const barHeight = Math.max(1, peak * maxBarHeight);
      const x = i * barWidth;
      // Centered vertically — symmetrical waveform.
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
        className={`group relative h-1.5 ${PLAYBACK_BAR_SEEK_RAIL_MIN_WIDTH_CLASS} flex-1 rounded-full bg-[var(--color-border)]`}
        onMouseDown={handleMouseDown}
        role="slider"
        aria-label={t("player.seek")}
        aria-valuemin={0}
        aria-valuemax={durationMs}
        aria-valuenow={Math.round(displayMs)}
        aria-valuetext={formatDuration(displayMs)}
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
          {/* Playhead dot — visible on hover and during drag */}
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
