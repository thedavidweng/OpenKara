import { useState } from "react";
import { useTranslation } from "react-i18next";
import { AlignLeft, Edit2, Languages, LoaderCircle } from "lucide-react";
import { Tooltip } from "@/components/Overlay/Tooltip";
import type { LyricsAlignment } from "@/lib/lyrics-session";
import { LyricsEditDialog } from "./LyricsEditDialog";

const ACTIVE_BUTTON_CLASS =
  "border-[color-mix(in_srgb,var(--color-accent)_40%,var(--color-border-light))] bg-[color-mix(in_srgb,var(--color-accent)_18%,var(--color-sidebar))] text-[var(--color-control-primary)]";

const IDLE_BUTTON_CLASS =
  "border-[var(--color-border-light)] bg-[var(--color-sidebar)] text-[var(--color-text-dim)] hover:border-[color-mix(in_srgb,var(--color-accent)_28%,var(--color-border-light))] hover:bg-[var(--color-hover)] hover:text-[var(--color-control-primary)]";

const BUTTON_CLASS =
  "motion-icon-button rounded-full border p-2 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50";

interface LyricsPanelToolbarProps {
  songId: string;
  rawLrc: string;
  pinned: boolean;
  showRomanized: boolean;
  isRomanizing: boolean;
  onToggleRomanized: () => void;
  alignment: LyricsAlignment;
  onToggleAlignment: () => void;
}

export function LyricsPanelToolbar({
  songId,
  rawLrc,
  pinned,
  showRomanized,
  isRomanizing,
  onToggleRomanized,
  alignment,
  onToggleAlignment,
}: LyricsPanelToolbarProps) {
  const { t } = useTranslation();
  const [editOpen, setEditOpen] = useState(false);
  const alignmentLabel =
    alignment === "left"
      ? t("lyrics.switchToCentered")
      : t("lyrics.switchToLeftAligned");

  return (
    <>
      <div
        className="contextual-reveal absolute right-4 top-4 z-10 flex gap-2"
        data-visible={pinned}
      >
        <Tooltip label={t("lyrics.romanizeTooltip")}>
          <button
            type="button"
            onClick={onToggleRomanized}
            aria-label={t("lyrics.romanizeTooltip")}
            disabled={isRomanizing}
            className={`${BUTTON_CLASS} ${
              showRomanized ? ACTIVE_BUTTON_CLASS : IDLE_BUTTON_CLASS
            }`}
          >
            {isRomanizing ? (
              <LoaderCircle size={14} className="animate-spin" />
            ) : (
              <Languages size={14} />
            )}
          </button>
        </Tooltip>
        <Tooltip label={t("lyrics.editTooltip")}>
          <button
            type="button"
            onClick={() => setEditOpen(true)}
            aria-label={t("lyrics.editTooltip")}
            className={`${BUTTON_CLASS} ${IDLE_BUTTON_CLASS}`}
          >
            <Edit2 size={14} />
          </button>
        </Tooltip>
        <Tooltip label={alignmentLabel}>
          <button
            type="button"
            onClick={onToggleAlignment}
            aria-label={alignmentLabel}
            className={`${BUTTON_CLASS} ${
              alignment === "left" ? ACTIVE_BUTTON_CLASS : IDLE_BUTTON_CLASS
            }`}
          >
            <AlignLeft size={14} />
          </button>
        </Tooltip>
      </div>
      <LyricsEditDialog
        open={editOpen}
        onClose={() => setEditOpen(false)}
        songId={songId}
        existingLyrics={rawLrc || undefined}
      />
    </>
  );
}
