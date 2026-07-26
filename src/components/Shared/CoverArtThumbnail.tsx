import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useCoverArtUrl } from "@/lib/cover-art";
import { getCoverArt, getCoverArtThumbnail } from "@/lib/tauri/library";
import type { CoverArtBytes } from "@/types/ipc";

interface CoverArtThumbnailProps {
  songHash: string;
  coverArt?: CoverArtBytes | null;
  /**
   * Absolute path of the on-disk 80x80 WebP derivative, from
   * `Song.artwork_thumb_path`. When present the image is served straight off
   * disk through the asset protocol, so the grid pays no IPC round trip, no
   * byte copy, and no blob lifetime bookkeeping per row.
   */
  thumbnailPath?: string | null;
  alt: string;
  className?: string;
}

export function CoverArtThumbnail({
  songHash,
  coverArt,
  thumbnailPath,
  alt,
  className = "",
}: CoverArtThumbnailProps) {
  const [fetchedBytes, setFetchedBytes] = useState<CoverArtBytes | null>(null);

  // Derivative filenames are content-addressed (`thumb_<sha256>_80.webp`), so a
  // replaced cover arrives as a different path and the WebView can cache the
  // image freely. A failed load means the file is gone or unreadable; drop back
  // to IPC, which is also what triggers the Rust-side lazy regeneration.
  const [brokenPath, setBrokenPath] = useState<string | null>(null);
  const assetUrl =
    thumbnailPath && thumbnailPath !== brokenPath
      ? convertFileSrc(thumbnailPath)
      : null;

  // Fetch on-demand when neither the asset path nor the bytes are available
  // (list/search results only carry has_cover_art, not the BLOB itself). Prefer
  // the 80x80 WebP thumbnail derivative; fall back to the full cover art.
  useEffect(() => {
    if (coverArt != null || assetUrl) return;
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
  }, [songHash, coverArt, assetUrl]);

  const blobUrl = useCoverArtUrl(
    songHash,
    assetUrl ? null : (coverArt ?? fetchedBytes),
    "thumb",
  );
  const url = assetUrl ?? blobUrl;
  const [failedUrl, setFailedUrl] = useState<string | null>(null);

  return (
    <div
      className={`overflow-hidden rounded-[10px] border border-[color-mix(in_srgb,var(--color-border)_82%,transparent)] bg-[var(--color-surface-muted)] ${className}`}
    >
      {/* RATIONALE (f4bc3ca): rendered eagerly, never `loading="lazy"` or
          `decoding="async"`. Desktop WebViews left blob-backed thumbnails
          unpainted with deferred decoding. Lazy loading would buy nothing here
          anyway — the library list is virtualized, so off-screen rows are not
          in the DOM to begin with. */}
      {url && failedUrl !== url ? (
        <img
          src={url}
          alt={alt}
          onError={() => {
            if (assetUrl && url === assetUrl && thumbnailPath) {
              setBrokenPath(thumbnailPath);
              return;
            }
            setFailedUrl(url);
          }}
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
