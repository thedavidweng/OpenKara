import { useEffect, useMemo } from "react";
import type { CoverArtBytes, CoverArtSize } from "@/types/ipc";

interface CoverArtCacheEntry {
  refs: number;
  url: string;
  byteDigest: string;
}

const coverArtUrlCache = new Map<string, CoverArtCacheEntry>();

function ensureCoverArtBytes(
  input: CoverArtBytes,
): Uint8Array<ArrayBuffer> | null {
  if (!input) {
    return null;
  }

  if (input instanceof ArrayBuffer) {
    return new Uint8Array(input);
  }

  if (ArrayBuffer.isView(input)) {
    return Uint8Array.from(
      new Uint8Array(input.buffer, input.byteOffset, input.byteLength),
    );
  }

  if (Array.isArray(input)) {
    return Uint8Array.from(input);
  }

  return null;
}

function byteDigest(bytes: Uint8Array): string {
  let hash1 = 0x811c9dc5;
  let hash2 = 0x89 ^ 0x1d;
  for (let i = 0; i < bytes.length; i++) {
    hash1 ^= bytes[i];
    hash1 = Math.imul(hash1, 0x01000193);
    hash2 ^= bytes[i];
    hash2 = Math.imul(hash2, 0x01000193);
  }
  return `${(hash1 >>> 0).toString(16)}:${(hash2 >>> 0).toString(16)}:${bytes.length}`;
}

export function detectCoverArtMime(bytes: CoverArtBytes): string {
  const normalizedBytes = ensureCoverArtBytes(bytes);
  if (!normalizedBytes || normalizedBytes.byteLength === 0) {
    return "image/jpeg";
  }

  if (normalizedBytes[0] === 0xff && normalizedBytes[1] === 0xd8) {
    return "image/jpeg";
  }
  if (
    normalizedBytes[0] === 0x89 &&
    normalizedBytes[1] === 0x50 &&
    normalizedBytes[2] === 0x4e &&
    normalizedBytes[3] === 0x47
  ) {
    return "image/png";
  }
  if (
    normalizedBytes[0] === 0x47 &&
    normalizedBytes[1] === 0x49 &&
    normalizedBytes[2] === 0x46
  ) {
    return "image/gif";
  }
  if (
    normalizedBytes[0] === 0x52 &&
    normalizedBytes[1] === 0x49 &&
    normalizedBytes[2] === 0x46
  ) {
    return "image/webp";
  }
  return "image/jpeg";
}

function cacheKey(songHash: string, size: CoverArtSize): string {
  return `${songHash}:${size}`;
}

export function retainCoverArtUrl(
  songHash: string,
  bytes: CoverArtBytes,
  size: CoverArtSize = "original",
): string | null {
  const normalizedBytes = ensureCoverArtBytes(bytes);

  if (
    !normalizedBytes ||
    normalizedBytes.byteLength === 0 ||
    typeof URL === "undefined" ||
    typeof URL.createObjectURL !== "function"
  ) {
    return null;
  }

  const key = cacheKey(songHash, size);
  const incomingDigest = byteDigest(normalizedBytes);
  const cached = coverArtUrlCache.get(key);
  if (cached) {
    if (cached.byteDigest !== incomingDigest) {
      if (typeof URL.revokeObjectURL === "function") {
        URL.revokeObjectURL(cached.url);
      }
      const url = URL.createObjectURL(
        new Blob([normalizedBytes], {
          type: detectCoverArtMime(normalizedBytes),
        }),
      );
      coverArtUrlCache.set(key, {
        refs: cached.refs,
        url,
        byteDigest: incomingDigest,
      });
      return url;
    }
    cached.refs += 1;
    return cached.url;
  }

  const url = URL.createObjectURL(
    new Blob([normalizedBytes], { type: detectCoverArtMime(normalizedBytes) }),
  );

  coverArtUrlCache.set(key, {
    refs: 1,
    url,
    byteDigest: incomingDigest,
  });

  return url;
}

export function releaseCoverArtUrl(
  songHash: string,
  size: CoverArtSize = "original",
): void {
  const key = cacheKey(songHash, size);
  const cached = coverArtUrlCache.get(key);
  if (!cached) {
    return;
  }

  cached.refs -= 1;
  if (cached.refs > 0) {
    return;
  }

  if (typeof URL !== "undefined" && typeof URL.revokeObjectURL === "function") {
    URL.revokeObjectURL(cached.url);
  }
  coverArtUrlCache.delete(key);
}

export function invalidateCoverArtUrl(songHash: string): void {
  for (const [key, entry] of coverArtUrlCache) {
    if (!key.startsWith(`${songHash}:`)) {
      continue;
    }
    if (
      typeof URL !== "undefined" &&
      typeof URL.revokeObjectURL === "function"
    ) {
      URL.revokeObjectURL(entry.url);
    }
    coverArtUrlCache.delete(key);
  }
}

export function resetCoverArtCacheForTests(): void {
  for (const [, entry] of coverArtUrlCache) {
    if (
      typeof URL !== "undefined" &&
      typeof URL.revokeObjectURL === "function"
    ) {
      URL.revokeObjectURL(entry.url);
    }
  }
  coverArtUrlCache.clear();
}

export function useCoverArtUrl(
  songHash: string,
  bytes: CoverArtBytes,
  size: CoverArtSize = "original",
): string | null {
  const url = useMemo(
    () => retainCoverArtUrl(songHash, bytes, size),
    [songHash, bytes, size],
  );

  useEffect(() => {
    if (!url) {
      return;
    }

    return () => {
      releaseCoverArtUrl(songHash, size);
    };
  }, [songHash, size, url]);

  return url;
}
