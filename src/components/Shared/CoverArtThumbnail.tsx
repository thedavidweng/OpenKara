import { useEffect, useState } from "react";
import { useCoverArtUrl } from "@/lib/cover-art";
import { getCoverArt } from "@/lib/tauri/library";
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
  // only carry has_cover_art, not the BLOB itself).
  useEffect(() => {
    if (coverArt != null) return;
    let cancelled = false;
    getCoverArt(songHash)
      .then((data) => {
        if (!cancelled) setFetchedBytes(data);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [songHash, coverArt]);

  const url = useCoverArtUrl(songHash, effectiveBytes);
  const [failedUrl, setFailedUrl] = useState<string | null>(null);

  return (
    <div
      className={`overflow-hidden rounded-[10px] border border-[color-mix(in_srgb,var(--color-border)_82%,transparent)] bg-[linear-gradient(180deg,color-mix(in_srgb,var(--color-control-primary)_10%,transparent),color-mix(in_srgb,var(--color-control-primary)_4%,transparent))] ${className}`}
    >
      {url && failedUrl !== url ? (
        <img
          src={url}
          alt={alt}
          onError={() => setFailedUrl(url)}
          className="block h-full w-full object-cover"
        />
      ) : (
        <div className="flex h-full w-full items-center justify-center bg-[radial-gradient(circle_at_top,color-mix(in_srgb,var(--color-control-primary)_22%,transparent),color-mix(in_srgb,var(--color-control-primary)_6%,transparent)_58%,rgba(0,0,0,0.12))]">
          <span className="h-2.5 w-2.5 rounded-full bg-white/28" aria-hidden />
        </div>
      )}
    </div>
  );
}
