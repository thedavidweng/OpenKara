import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { Monitor } from "lucide-react";
import { buildAudiencePresentationSpec } from "@/lib/audience-presentation";
import { getMonitors, openFullscreenPlayer } from "@/lib/fullscreen-player";
import { syncAirPlayAudienceState } from "@/lib/tauri";

interface MonitorInfo {
  name: string;
  width: number;
  height: number;
}

interface MonitorPickerProps {
  onClose: () => void;
  anchorRef: React.RefObject<HTMLButtonElement | null>;
}

interface MonitorPickerPosition {
  left: number;
  top: number;
  minWidth: number;
}

const MONITOR_PICKER_MIN_WIDTH = 200;
const MONITOR_PICKER_OFFSET = 8;
const VIEWPORT_PADDING = 12;

function getMonitorPickerPosition(
  anchorRect: DOMRect,
  viewportWidth: number,
  viewportHeight: number,
): MonitorPickerPosition {
  const minWidth = Math.max(MONITOR_PICKER_MIN_WIDTH, anchorRect.width);
  const maxLeft = Math.max(
    VIEWPORT_PADDING,
    viewportWidth - minWidth - VIEWPORT_PADDING,
  );

  return {
    left: Math.min(
      Math.max(anchorRect.right - minWidth, VIEWPORT_PADDING),
      maxLeft,
    ),
    top: Math.min(
      anchorRect.bottom + MONITOR_PICKER_OFFSET,
      viewportHeight - VIEWPORT_PADDING,
    ),
    minWidth,
  };
}

function normalizeMonitors(
  monitors: Awaited<ReturnType<typeof getMonitors>>,
): MonitorInfo[] {
  return monitors.map((monitor) => ({
    name: monitor.name ?? "Display",
    width: monitor.size.width,
    height: monitor.size.height,
  }));
}

export function MonitorPicker({ onClose, anchorRef }: MonitorPickerProps) {
  const { t } = useTranslation();
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  const [position, setPosition] = useState<MonitorPickerPosition | null>(null);
  const [focusedIndex, setFocusedIndex] = useState(0);
  const menuRef = useRef<HTMLDivElement>(null);
  const optionRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const didInitialFocus = useRef(false);

  useEffect(() => {
    let cancelled = false;

    getMonitors()
      .then((next) => {
        if (!cancelled) {
          setMonitors(normalizeMonitors(next));
        }
      })
      .catch(() => {
        if (!cancelled) {
          setMonitors([]);
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const anchor = anchorRef.current;
    if (anchor) {
      anchor.setAttribute("aria-haspopup", "listbox");
      anchor.setAttribute("aria-expanded", "true");
    }
    return () => {
      if (anchor) {
        anchor.setAttribute("aria-expanded", "false");
      }
    };
  }, [anchorRef]);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        menuRef.current &&
        !menuRef.current.contains(event.target as Node) &&
        anchorRef.current &&
        !anchorRef.current.contains(event.target as Node)
      ) {
        onClose();
      }
    };
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        anchorRef.current?.focus();
        onClose();
      }
    };

    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleEscape);

    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleEscape);
    };
  }, [anchorRef, onClose]);

  useEffect(() => {
    if (monitors.length > 0 && !didInitialFocus.current) {
      didInitialFocus.current = true;
      setFocusedIndex(0);
      optionRefs.current[0]?.focus();
    }
  }, [monitors]);

  useEffect(() => {
    optionRefs.current[focusedIndex]?.focus();
  }, [focusedIndex]);

  useLayoutEffect(() => {
    const updatePosition = () => {
      const anchor = anchorRef.current;
      if (!anchor) {
        setPosition(null);
        return;
      }

      setPosition(
        getMonitorPickerPosition(
          anchor.getBoundingClientRect(),
          window.innerWidth,
          window.innerHeight,
        ),
      );
    };

    updatePosition();
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);

    return () => {
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
    };
  }, [anchorRef]);

  if (!position) {
    return null;
  }

  const handleMonitorSelect = async (index: number) => {
    await syncAirPlayAudienceState({
      mode: "idle",
      songId: null,
      lines: [],
      offsetMs: 0,
      isLoading: false,
      lyricsFontStep: 0,
      messages: {
        selectSong: t("lyrics.selectSong"),
        loadingLyrics: t("lyrics.loadingLyrics"),
        noLyrics: t("lyrics.noLyrics"),
        addLyrics: t("lyrics.addLyrics"),
      },
      viewport: {
        width_px: 1280,
        height_px: 720,
        bottom_inset_px: 0,
      },
      presentationSpec: buildAudiencePresentationSpec(0),
    }).catch(() => {});

    openFullscreenPlayer(index);
    onClose();
  };

  const handleListboxKeyDown = (e: React.KeyboardEvent) => {
    if (monitors.length === 0) return;
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setFocusedIndex((prev) => (prev + 1) % monitors.length);
        break;
      case "ArrowUp":
        e.preventDefault();
        setFocusedIndex(
          (prev) => (prev - 1 + monitors.length) % monitors.length,
        );
        break;
      case "Home":
        e.preventDefault();
        setFocusedIndex(0);
        break;
      case "End":
        e.preventDefault();
        setFocusedIndex(monitors.length - 1);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        void handleMonitorSelect(focusedIndex);
        break;
      case "Escape":
        e.preventDefault();
        e.stopPropagation();
        anchorRef.current?.focus();
        onClose();
        break;
    }
  };

  return createPortal(
    <div
      ref={menuRef}
      className="app-panel-surface fixed z-[70] rounded-lg border border-[var(--color-border)] bg-[color-mix(in_srgb,var(--color-sidebar)_94%,transparent)] p-1 shadow-[0_20px_40px_rgba(0,0,0,0.32)]"
      style={{
        left: position.left,
        top: position.top,
        minWidth: position.minWidth,
      }}
    >
      <div className="px-2 py-1 text-[11px] font-semibold text-[var(--color-text-dim)]">
        {t("player.selectMonitor")}
      </div>
      <div className="px-2 py-1 text-[11px] font-semibold text-[var(--color-text-dim)]">
        {t("player.localDisplayOutput")}
      </div>
      {monitors.length === 0 ? (
        <div className="px-2 py-2 text-[11px] text-[var(--color-text-dim)]">
          {t("player.noDisplaysFound")}
        </div>
      ) : null}
      <div
        role="listbox"
        aria-label={t("player.localDisplayOutput")}
        tabIndex={-1}
        onKeyDown={handleListboxKeyDown}
      >
        {monitors.map((monitor, index) => (
          <button
            key={`${monitor.name}-${monitor.width}-${monitor.height}-${index}`}
            ref={(el) => {
              optionRefs.current[index] = el;
            }}
            type="button"
            role="option"
            aria-selected={index === focusedIndex}
            tabIndex={index === focusedIndex ? 0 : -1}
            onClick={() => {
              void handleMonitorSelect(index);
            }}
            onMouseEnter={() => setFocusedIndex(index)}
            onFocus={() => setFocusedIndex(index)}
            className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text)]"
          >
            <Monitor size={14} className="text-[var(--color-text-dim)]" />
            <div className="min-w-0 flex-1">
              <div className="truncate">{monitor.name}</div>
              <div className="text-[10px] text-[var(--color-text-dimmer)]">
                {t("player.monitor", { index: index + 1 })}
              </div>
            </div>
            <span className="ml-auto text-[10px] text-[var(--color-text-dimmer)]">
              {monitor.width}x{monitor.height}
            </span>
          </button>
        ))}
      </div>
    </div>,
    document.body,
  );
}
