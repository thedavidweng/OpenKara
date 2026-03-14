const blobUrlCache = new Map<string, string>();

function detectMime(bytes: number[]): string {
  if (bytes[0] === 0xff && bytes[1] === 0xd8) return "image/jpeg";
  if (
    bytes[0] === 0x89 &&
    bytes[1] === 0x50 &&
    bytes[2] === 0x4e &&
    bytes[3] === 0x47
  )
    return "image/png";
  if (bytes[0] === 0x47 && bytes[1] === 0x49 && bytes[2] === 0x46)
    return "image/gif";
  if (bytes[0] === 0x52 && bytes[1] === 0x49 && bytes[2] === 0x46)
    return "image/webp";
  return "image/jpeg";
}

export function getCoverArtUrl(
  songHash: string,
  bytes: number[] | null,
): string | null {
  if (!bytes || bytes.length === 0) return null;

  const cached = blobUrlCache.get(songHash);
  if (cached) return cached;

  const mime = detectMime(bytes);
  const blob = new Blob([new Uint8Array(bytes)], { type: mime });
  const url = URL.createObjectURL(blob);
  blobUrlCache.set(songHash, url);
  return url;
}

export function revokeCoverArtUrl(songHash: string): void {
  const url = blobUrlCache.get(songHash);
  if (url) {
    URL.revokeObjectURL(url);
    blobUrlCache.delete(songHash);
  }
}
