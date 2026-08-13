import type { ReactNode } from "react";
import { colorToCss } from "@/lib/audience-presentation";
import type { AudiencePresentationSpec } from "@/types/ipc";

interface LyricsStatusMessageProps {
  isAudience: boolean;
  audienceSpec: AudiencePresentationSpec;
  className: string;
  children: ReactNode;
}

export function LyricsStatusMessage({
  isAudience,
  audienceSpec,
  className,
  children,
}: LyricsStatusMessageProps) {
  return (
    <div className="flex flex-1 items-center justify-center">
      <p
        className={className}
        style={
          isAudience
            ? {
                fontSize: audienceSpec.statusFontSizePx,
                color: colorToCss(audienceSpec.statusTextColor),
              }
            : { fontSize: 14 }
        }
      >
        {children}
      </p>
    </div>
  );
}
