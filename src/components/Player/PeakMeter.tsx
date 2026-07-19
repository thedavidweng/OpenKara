import { useEffect, useRef } from "react";
import { getAudioPeaks } from "@/lib/tauri/playback";
import type { AudioPeakSnapshot } from "@/types/ipc";

/**
 * Realtime stereo peak envelope visualizer.
 *
 * Polls the Rust `get_audio_peaks` command at 30 Hz and renders the latest
 * peak pairs on a DPR-aware canvas. The backend publishes one pair per 512
 * rendered frames; the ring retains the last 256 pairs (~3 s at 44.1 kHz).
 *
 * The canvas is purely a visual observability channel — it never blocks
 * playback and degrades gracefully when no audio is playing (flat line).
 */
export function PeakMeter({
  width = 240,
  height = 40,
  barGap = 2,
}: {
  width?: number;
  height?: number;
  barGap?: number;
} = {}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const lastWriteIndexRef = useRef(0);
  // Timestamp of the last writeIndex advance. When the index stops moving
  // (playback paused/stopped) the backend keeps returning the same non-empty
  // snapshot, so we use elapsed time to fall back to the flat-line state.
  const lastAdvanceRef = useRef<number | null>(null);
  // Whether the flat-line state has already been drawn. Prevents redundant
  // 30 Hz redraws when playback is idle — the canvas content is static so
  // there is nothing to repaint until new peaks arrive.
  const flatLineDrawnRef = useRef(false);
  // Last non-empty snapshot, captured so we can animate a smooth decay to
  // the flat line instead of an abrupt jump when playback pauses/stops.
  const lastPeaksRef = useRef<AudioPeakSnapshot | null>(null);
  // When decay started (performance.now ms). null while not decaying.
  const decayStartRef = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setInterval> | null = null;
    // Monotonic generation so a slow getAudioPeaks response cannot overwrite
    // a newer poll (or apply after unmount/interval rebuild).
    let requestGeneration = 0;
    // Single-flight coordination: at most one getAudioPeaks() call in flight
    // per effect lifetime. Ticks that arrive while in flight coalesce into a
    // single follow-up poll (rerunRequested) instead of stacking concurrent
    // IPC calls whose responses would all be invalidated by newer ticks.
    let inFlight = false;
    let rerunRequested = false;

    const draw = (snapshot: AudioPeakSnapshot) => {
      const canvas = canvasRef.current;
      if (!canvas) return;

      const ctx = canvas.getContext("2d");
      if (!ctx) return;

      const dpr = window.devicePixelRatio || 1;
      const physicalWidth = Math.round(width * dpr);
      const physicalHeight = Math.round(height * dpr);

      if (canvas.width !== physicalWidth || canvas.height !== physicalHeight) {
        canvas.width = physicalWidth;
        canvas.height = physicalHeight;
      }

      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, width, height);

      const peaks = snapshot.peaks;
      if (peaks.length === 0) {
        const midY = height / 2;
        ctx.fillStyle = "rgba(255, 255, 255, 0.15)";
        ctx.fillRect(0, midY - 0.5, width, 1);
        return;
      }

      const barWidth = 3;
      const maxBars = Math.floor((width + barGap) / (barWidth + barGap));
      // Take the most recent peaks (right-aligned, scrolling).
      const startIdx = Math.max(0, peaks.length - maxBars);
      const visiblePeaks = peaks.slice(startIdx);
      const barCount = visiblePeaks.length;
      const totalWidth = barCount * barWidth + (barCount - 1) * barGap;
      const startX = width - totalWidth;

      const midY = height / 2;
      const maxBarHeight = (height - 2) / 2;

      for (let i = 0; i < visiblePeaks.length; i++) {
        const [left, right] = visiblePeaks[i];
        const x = startX + i * (barWidth + barGap);

        const leftH = Math.max(1, left * maxBarHeight);
        ctx.fillStyle = "rgba(255, 255, 255, 0.7)";
        ctx.fillRect(x, midY - leftH, barWidth, leftH);

        const rightH = Math.max(1, right * maxBarHeight);
        ctx.fillStyle = "rgba(255, 255, 255, 0.45)";
        ctx.fillRect(x, midY, barWidth, rightH);
      }
    };

    const poll = async () => {
      if (cancelled) return;
      // Single-flight guard: if a previous poll is still in flight, coalesce
      // the tick into a single follow-up poll instead of issuing another IPC
      // call. Without this, a slow backend causes every timer tick to start a
      // concurrent request, and each response is invalidated by the next tick,
      // so the meter can stop rendering entirely while spinning on IPC.
      if (inFlight) {
        rerunRequested = true;
        return;
      }
      inFlight = true;
      const generation = ++requestGeneration;
      try {
        const snapshot = await getAudioPeaks();
        // Drop stale responses: a later poll already started, or we unmounted.
        if (cancelled || generation !== requestGeneration) return;
        const now = performance.now();
        // Monotonic comparison: an older write index (e.g. from a reordered
        // or wrapped response) must never replace a newer canvas state.
        const advanced = snapshot.writeIndex > lastWriteIndexRef.current;
        if (advanced) {
          lastWriteIndexRef.current = snapshot.writeIndex;
          lastAdvanceRef.current = now;
          flatLineDrawnRef.current = false;
          decayStartRef.current = null;
          lastPeaksRef.current = snapshot;
          draw(snapshot);
        } else if (snapshot.peaks.length === 0) {
          // Only draw once — the canvas content is static until peaks arrive.
          if (!flatLineDrawnRef.current) {
            flatLineDrawnRef.current = true;
            draw(snapshot);
          }
        } else {
          // writeIndex unchanged with non-empty peaks: playback has likely
          // stopped or paused. After a grace period, animate a smooth decay
          // from the last waveform to the flat line so the transition is not
          // an abrupt jump. The decay runs over 400 ms at the 30 Hz poll
          // cadence, scaling the last peak values toward zero.
          //
          // If the component mounted with non-empty peaks whose writeIndex
          // never advanced from the initial value (e.g. writeIndex=0 with
          // data, or a writeIndex equal to the initial lastWriteIndexRef),
          // lastAdvanceRef stays null and the staleness check below would
          // never fire. Start the grace period on the first static non-empty
          // poll so the canvas eventually flat-lines.
          if (lastAdvanceRef.current === null) {
            lastAdvanceRef.current = now;
          }
          const elapsed = now - lastAdvanceRef.current;
          const graceMs = 500;
          const decayMs = 400;
          if (elapsed <= graceMs) {
            if (!lastPeaksRef.current) lastPeaksRef.current = snapshot;
          } else if (decayStartRef.current === null) {
            decayStartRef.current = now;
          }
          if (decayStartRef.current !== null) {
            const decayElapsed = now - decayStartRef.current;
            if (decayElapsed >= decayMs) {
              if (!flatLineDrawnRef.current) {
                flatLineDrawnRef.current = true;
                draw({ ...snapshot, peaks: [] });
              }
            } else {
              // Animate: scale the last peaks toward zero with an
              // ease-out curve so the bars settle smoothly.
              const progress = decayElapsed / decayMs;
              const factor = 1 - Math.pow(progress, 2);
              const base = lastPeaksRef.current ?? snapshot;
              const decayed = base.peaks.map(
                ([l, r]) => [l * factor, r * factor] as [number, number],
              );
              draw({ ...base, peaks: decayed });
            }
          }
        }
      } catch {
        // Backend may be unavailable during startup — silently skip.
      } finally {
        inFlight = false;
        // If a tick arrived while this poll was in flight, run exactly one
        // follow-up poll so we don't lose a cadence cycle to coalescing.
        if (!cancelled && rerunRequested) {
          rerunRequested = false;
          void poll();
        }
      }
    };

    void poll();
    timer = setInterval(poll, 1000 / 30);

    return () => {
      cancelled = true;
      if (timer) clearInterval(timer);
    };
  }, [width, height, barGap]);

  return (
    <canvas
      ref={canvasRef}
      style={{ width, height }}
      className="block"
      aria-hidden="true"
      data-peak-meter
    />
  );
}
