export interface WaveformCacheEntry {
  peaks: number[];
  buckets: number;
}

const WAVEFORM_LRU_CAPACITY = 96;

const lruOrder: string[] = [];
const lruMap = new Map<string, WaveformCacheEntry>();

export function waveformCacheKey(songHash: string, buckets: number): string {
  return `${songHash}:${buckets}`;
}

export function bucketsForRailWidth(cssWidth: number, dpr: number): number {
  if (
    !Number.isFinite(cssWidth) ||
    cssWidth <= 0 ||
    !Number.isFinite(dpr) ||
    dpr <= 0
  ) {
    return 200;
  }
  return Math.min(1000, Math.max(24, Math.round((cssWidth * dpr) / 3)));
}

export function getWaveformCache(
  songHash: string,
  buckets: number,
): WaveformCacheEntry | null {
  const key = waveformCacheKey(songHash, buckets);
  const entry = lruMap.get(key);
  if (!entry) {
    return null;
  }
  const idx = lruOrder.indexOf(key);
  if (idx >= 0) {
    lruOrder.splice(idx, 1);
  }
  lruOrder.push(key);
  return entry;
}

export function setWaveformCache(
  songHash: string,
  buckets: number,
  peaks: number[],
): void {
  if (peaks.length === 0) {
    return;
  }
  const key = waveformCacheKey(songHash, buckets);
  if (lruMap.has(key)) {
    const idx = lruOrder.indexOf(key);
    if (idx >= 0) {
      lruOrder.splice(idx, 1);
    }
  }
  lruMap.set(key, { peaks, buckets });
  lruOrder.push(key);
  while (lruOrder.length > WAVEFORM_LRU_CAPACITY) {
    const oldest = lruOrder.shift();
    if (oldest) {
      lruMap.delete(oldest);
    }
  }
}

export function resetWaveformCacheForTests(): void {
  lruOrder.length = 0;
  lruMap.clear();
}

export function waveformCacheSizeForTests(): number {
  return lruMap.size;
}
