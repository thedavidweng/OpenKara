import { useEffect, useState } from "react";
import { useCoverArtUrl } from "@/lib/cover-art";
import { getCoverArt, getCoverArtThumbnail } from "@/lib/tauri/library";
import type { CoverArtBytes } from "@/types/ipc";

interface CoverArtThumbnailProps {
  songHash: string;
  coverArt?: CoverArtBytes | null;
  alt: string;
  className?: string;
}

export function CoverArtThumbnail({
  songHash,
  coverArt,
  alt,
  className = "",
}: CoverArtThumbnailProps) {
  const [fetchedBytes, setFetchedBytes] = useState<CoverArtBytes | null>(null);
  const effectiveBytes = coverArt ?? fetchedBytes;

  // Fetch on-demand when cover art bytes are not provided (list/search results
  // only carry has_cover_art, not the BLOB itself). Prefer the 80×80 WebP
  // thumbnail derivative (cheaper IPC payload + faster decode); fall back to
  // the full cover art if the derivative is unavailable.
  useEffect(() => {
    if (coverArt != null) return;
    let cancelled = false;
    (async () => {
      try {
        const thumb = await getCoverArtThumbnail(songHash);
        if (cancelled) return;
        if (thumb) {
          setFetchedBytes(thumb);
          return;
        }
        const full = await getCoverArt(songHash);
        if (cancelled) return;
        setFetchedBytes(full);
      } catch {
        // ignore — the placeholder will render
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [songHash, coverArt]);

  const url = useCoverArtUrl(songHash, effectiveBytes, "thumb");
  const [failedUrl, setFailedUrl] = useState<string | null>(null);

  return (
    <div
      className={`overflow-hidden rounded-[10px] border border-[color-mix(in_srgb,var(--color-border)_82%,transparent)] bg-[var(--color-surface-muted)] ${className}`}
    >
      {url && failedUrl !== url ? (
        <img
          src={url}
          alt={alt}
          onError={() => setFailedUrl(url)}
          className="block h-full w-full object-cover"
        />
      ) : (
        <div className="flex h-full w-full items-center justify-center bg-[var(--color-surface-muted)]">
          <span
            className="h-2.5 w-2.5 rounded-full bg-[var(--color-text-dimmer)]"
            aria-hidden
          />
        </div>
      )}
    </div>
  );
}
