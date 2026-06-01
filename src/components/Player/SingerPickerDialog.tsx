import { useEffect } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

interface SingerPickerDialogProps {
  singerNames: string[];
  currentSinger: string | null;
  onSelect: (singer: string) => void;
  onRemove: () => void;
  onCancel: () => void;
}

export function SingerPickerDialog({
  singerNames,
  currentSinger,
  onSelect,
  onRemove,
  onCancel,
}: SingerPickerDialogProps) {
  const { t } = useTranslation();

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onCancel();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onCancel]);

  const dialogContent = (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div className="absolute inset-0 bg-black/60" onClick={onCancel} />

      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="singer-picker-title"
        className="relative w-full max-w-xs rounded-lg border border-[var(--color-border)] bg-[var(--color-sidebar)] p-4 shadow-xl"
      >
        <h3
          id="singer-picker-title"
          className="mb-3 text-[14px] font-semibold text-white"
        >
          {t("rotation.assignSinger")}
        </h3>

        <div className="flex flex-col gap-1">
          {singerNames.map((name) => (
            <button
              key={name}
              type="button"
              onClick={() => onSelect(name)}
              className={`rounded-md px-3 py-2 text-left text-[13px] transition-colors ${
                name === currentSinger
                  ? "bg-[var(--color-accent)] text-white"
                  : "text-[var(--color-text)] hover:bg-[var(--color-hover)] hover:text-white"
              }`}
            >
              {name}
            </button>
          ))}
        </div>

        {currentSinger && (
          <button
            type="button"
            onClick={onRemove}
            className="mt-2 w-full rounded-md border border-red-500/40 bg-red-600/10 px-3 py-2 text-[12px] text-red-400 transition-colors hover:bg-red-600/20 hover:text-red-300"
          >
            {t("rotation.removeSinger")}
          </button>
        )}

        <button
          type="button"
          onClick={onCancel}
          className="mt-2 w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-2 text-[13px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] hover:text-white"
        >
          {t("common.cancel")}
        </button>
      </div>
    </div>
  );

  if (typeof document === "undefined" || !document.body) {
    return dialogContent;
  }

  return createPortal(dialogContent, document.body);
}
