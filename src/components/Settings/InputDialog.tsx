import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

interface InputDialogProps {
  title: string;
  placeholder?: string;
  initialValue?: string;
  confirmLabel?: string;
  onConfirm: (value: string) => void;
  onCancel: () => void;
}

export function InputDialog({
  title,
  placeholder,
  initialValue = "",
  confirmLabel,
  onConfirm,
  onCancel,
}: InputDialogProps) {
  const { t } = useTranslation();
  const [value, setValue] = useState(initialValue);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onCancel();
      } else if (e.key === "Enter" && value.trim()) {
        onConfirm(value.trim());
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onCancel, onConfirm, value]);

  const label = confirmLabel || t("common.save");

  const dialogContent = (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      {/* Backdrop */}
      <div className="absolute inset-0 bg-black/60" onClick={onCancel} />

      {/* Dialog */}
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="input-dialog-title"
        className="relative w-full max-w-sm rounded-lg border border-[var(--color-border)] bg-[var(--color-sidebar)] p-6 shadow-xl"
      >
        <h3
          id="input-dialog-title"
          className="break-words text-[15px] font-semibold text-white"
        >
          {title}
        </h3>

        <input
          ref={inputRef}
          type="text"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder={placeholder}
          className="mt-3 w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-2 text-[13px] text-white placeholder-[var(--color-text-dimmer)] outline-none transition-colors focus:border-[var(--color-accent)] focus:ring-1 focus:ring-[var(--color-accent)]/30"
        />

        <div className="mt-5 flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-4 py-2 text-[13px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] hover:text-white focus:outline-none focus:ring-2 focus:ring-[var(--color-accent)]/30"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={() => value.trim() && onConfirm(value.trim())}
            disabled={!value.trim()}
            className="rounded-md bg-[var(--color-accent)] px-4 py-2 text-[13px] text-white transition-colors hover:bg-[var(--color-accent-hover)] focus:outline-none focus:ring-2 focus:ring-[var(--color-accent)]/40 disabled:opacity-50"
          >
            {label}
          </button>
        </div>
      </div>
    </div>
  );

  if (typeof document === "undefined" || !document.body) {
    return dialogContent;
  }

  return createPortal(dialogContent, document.body);
}
