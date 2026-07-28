import { useState, useRef } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { useModalDialog } from "@/hooks/use-modal-dialog";
import { DialogBackdrop } from "@/components/Overlay/DialogBackdrop";
import { useLibraryStore } from "@/stores/library-store";
import type { Song } from "@/types/ipc";

interface SongEditDialogProps {
  song: Song;
  onClose: () => void;
}

export function SongEditDialog({ song, onClose }: SongEditDialogProps) {
  const { t } = useTranslation();
  const [title, setTitle] = useState(song.title ?? "");
  const [artist, setArtist] = useState(song.artist ?? "");
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const updateSongMetadata = useLibraryStore((s) => s.updateSongMetadata);
  const dialogRef = useRef<HTMLDivElement>(null);
  const titleInputRef = useRef<HTMLInputElement>(null);
  const titleInputId = `song-title-${song.hash}`;
  const artistInputId = `song-artist-${song.hash}`;
  const headingId = `song-edit-heading-${song.hash}`;
  const errorId = `song-edit-error-${song.hash}`;

  useModalDialog({
    dialogRef,
    initialFocusRef: titleInputRef,
    onDismiss: onClose,
    canDismiss: !saving,
  });

  const handleSave = async () => {
    if (saving) return;

    setSaveError(null);
    setSaving(true);
    const saved = await updateSongMetadata(
      song.hash,
      title.trim() || null,
      artist.trim() || null,
    );
    if (saved) {
      onClose();
      return;
    }

    setSaveError(t("errors.somethingWentWrong"));
    setSaving(false);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !saving) {
      e.preventDefault();
      handleSave();
    }
  };

  if (typeof document === "undefined" || !document.body) {
    return null;
  }

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <DialogBackdrop
        ariaLabel={t("common.close")}
        onDismiss={() => {
          if (!saving) onClose();
        }}
        className="absolute inset-0 bg-black/50"
      />
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={headingId}
        aria-busy={saving}
        tabIndex={-1}
        className="relative w-full max-w-sm overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-sidebar)] shadow-2xl"
      >
        <div className="border-b border-[var(--color-border)] px-5 py-3">
          <h3
            id={headingId}
            className="text-[14px] font-semibold text-[var(--color-text)]"
          >
            {t("songEdit.title")}
          </h3>
        </div>
        <div className="space-y-4 p-5">
          <div className="space-y-1.5">
            <label
              htmlFor={titleInputId}
              className="text-[12px] font-medium text-[var(--color-text-dim)]"
            >
              {t("songEdit.titleLabel")}
            </label>
            <input
              ref={titleInputRef}
              id={titleInputId}
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={t("songEdit.titlePlaceholder")}
              className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-2 text-[13px] text-[var(--color-text)] placeholder:text-[var(--color-text-dimmer)] transition-colors focus:border-[var(--color-accent)] focus:outline-none focus:ring-2 focus:ring-[var(--color-accent)]/30"
            />
          </div>
          <div className="space-y-1.5">
            <label
              htmlFor={artistInputId}
              className="text-[12px] font-medium text-[var(--color-text-dim)]"
            >
              {t("songEdit.artistLabel")}
            </label>
            <input
              id={artistInputId}
              type="text"
              value={artist}
              onChange={(e) => setArtist(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={t("songEdit.artistPlaceholder")}
              className="w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-2 text-[13px] text-[var(--color-text)] placeholder:text-[var(--color-text-dimmer)] transition-colors focus:border-[var(--color-accent)] focus:outline-none focus:ring-2 focus:ring-[var(--color-accent)]/30"
            />
          </div>
          {saveError && (
            <p
              id={errorId}
              role="alert"
              className="break-words rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-[12px] text-[var(--color-destructive)]"
            >
              {saveError}
            </p>
          )}
        </div>
        <div className="flex justify-end gap-2 border-t border-[var(--color-border)] px-5 py-3">
          <button
            onClick={onClose}
            disabled={saving}
            className="rounded-md px-3 py-2 text-[12px] text-[var(--color-text-dim)] transition-colors hover:text-[var(--color-text)] focus:outline-none focus:ring-2 focus:ring-[var(--color-accent)]/30 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={handleSave}
            disabled={saving}
            aria-describedby={saveError ? errorId : undefined}
            className="rounded-md bg-[var(--color-control-primary)] px-3 py-2 text-[12px] font-medium text-[var(--color-control-primary-foreground)] transition-colors hover:bg-[color-mix(in_srgb,var(--color-control-primary)_88%,white)] focus:outline-none focus:ring-2 focus:ring-[var(--color-focus-ring)] disabled:cursor-not-allowed disabled:opacity-50"
          >
            {saving ? t("common.saving") : t("common.save")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
