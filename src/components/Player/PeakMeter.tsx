import { useEffect, useRef } from "react";
import { getAudioPeaks } from "@/lib/tauri/playback";
import type { AudioPeakSnapshot } from "@/types/ipc";

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
  const lastAdvanceRef = useRef<number | null>(null);
  const flatLineDrawnRef = useRef(false);
  const lastPeaksRef = useRef<AudioPeakSnapshot | null>(null);
  const decayStartRef = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setInterval> | null = null;
    let requestGeneration = 0;
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
      if (inFlight) {
        rerunRequested = true;
        return;
      }
      inFlight = true;
      const generation = ++requestGeneration;
      try {
        const snapshot = await getAudioPeaks();
        if (cancelled || generation !== requestGeneration) return;
        const now = performance.now();
        const advanced = snapshot.writeIndex > lastWriteIndexRef.current;
        if (advanced) {
          lastWriteIndexRef.current = snapshot.writeIndex;
          lastAdvanceRef.current = now;
          flatLineDrawnRef.current = false;
          decayStartRef.current = null;
          lastPeaksRef.current = snapshot;
          draw(snapshot);
        } else if (snapshot.peaks.length === 0) {
          if (!flatLineDrawnRef.current) {
            flatLineDrawnRef.current = true;
            draw(snapshot);
          }
        } else {
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
      } finally {
        inFlight = false;
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
