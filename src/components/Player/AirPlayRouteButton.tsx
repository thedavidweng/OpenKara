import { useLayoutEffect, useRef } from "react";
import { Airplay } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Tooltip } from "@/components/Overlay/Tooltip";
import { getShortcutPlatform } from "@/lib/app-shortcuts";
import { syncAirPlayRoutePicker } from "@/lib/tauri";
import type { AirPlayRoutePickerBounds } from "@/types/ipc";

function buildHostBounds(element: HTMLDivElement): AirPlayRoutePickerBounds {
  const rect = element.getBoundingClientRect();
  return {
    left: rect.left,
    top: rect.top,
    width: rect.width,
    height: rect.height,
  };
}

interface AirPlayRouteButtonProps {
  className?: string;
  /** Landing-page preview: preserve the AirPlay slot without mounting AppKit. */
  previewMode?: boolean;
}

export function AirPlayRouteButton({
  className = "h-8 w-8 flex items-center justify-center",
  previewMode = false,
}: AirPlayRouteButtonProps) {
  const { t } = useTranslation();
  const platform = previewMode ? "mac" : getShortcutPlatform();
  const hostRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (previewMode || platform !== "mac" || !hostRef.current) {
      return;
    }

    const host = hostRef.current;

    const syncBounds = () => {
      if (!hostRef.current) {
        return;
      }

      void syncAirPlayRoutePicker(buildHostBounds(hostRef.current)).catch(
        () => {},
      );
    };

    syncBounds();
    window.addEventListener("resize", syncBounds);

    const resizeObserver =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(syncBounds);
    resizeObserver?.observe(host);

    return () => {
      window.removeEventListener("resize", syncBounds);
      resizeObserver?.disconnect();
      void syncAirPlayRoutePicker(null).catch(() => {
        // Best effort teardown only.
      });
    };
  }, [platform, previewMode]);

  if (platform !== "mac") {
    return null;
  }

  if (previewMode) {
    return (
      <Tooltip label={t("player.airPlayOutput")}>
        <div
          className={`relative text-[var(--color-text-dim)] ${className}`}
          data-airplay-route-button="true"
          data-airplay-preview="true"
          aria-label={t("player.airPlayOutput")}
        >
          <Airplay size={16} aria-hidden="true" />
        </div>
      </Tooltip>
    );
  }

  return (
    <Tooltip label={t("player.airPlayOutput")}>
      <div
        className={`relative ${className}`}
        data-airplay-route-button="true"
        aria-label={t("player.airPlayOutput")}
      >
        <div
          ref={hostRef}
          className="h-full w-full rounded-[inherit]"
          data-airplay-route-host="true"
        />
      </div>
    </Tooltip>
  );
}
