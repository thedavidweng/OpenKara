import { useEffect, useState } from "react";
import { Music2 } from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";

interface StreamingCoverProps {
  url: string | null;
  /** Requested thumbnail edge in CSS pixels (doubled for dense displays). */
  size: number;
  className?: string;
  /** Desaturate the art (unavailable tracks). */
  muted?: boolean;
  /** Blurred copy of the art behind the cover (playlist header). */
  glow?: boolean;
  eager?: boolean;
}

/**
 * Square cover with a fixed aspect box, so nothing reflows while art loads.
 * Falls back to a neutral tile when there is no URL or the image fails.
 */
export function StreamingCover({
  url,
  size,
  className = "",
  muted = false,
  glow = false,
  eager = false,
}: StreamingCoverProps) {
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [url]);
  const src = failed || !url ? null : convertFileSrc(url);

  return (
    <div className={`relative aspect-square shrink-0 ${className}`}>
      {glow && src ? (
        <div
          aria-hidden="true"
          className="pointer-events-none absolute inset-x-0 top-3 -z-10 aspect-square rounded-xl bg-cover opacity-60 blur-[16px]"
          style={{
            backgroundImage: `url("${src}")`,
            transform: "scale(0.92, 0.96)",
          }}
        />
      ) : null}
      <div className="relative size-full overflow-hidden rounded-[inherit] border border-[var(--color-border)] bg-[var(--color-surface-muted)]">
        <span
          aria-hidden="true"
          className="absolute inset-0 flex items-center justify-center text-[var(--color-text-dim)]"
        >
          <Music2 className="size-[36%]" />
        </span>
        {src ? (
          <img
            src={src}
            alt=""
            width={size}
            height={size}
            loading={eager ? "eager" : "lazy"}
            decoding="async"
            draggable={false}
            onError={() => setFailed(true)}
            className={`absolute inset-0 size-full object-cover ${muted ? "grayscale" : ""}`}
          />
        ) : null}
      </div>
    </div>
  );
}
