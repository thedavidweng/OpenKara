import { useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { useModalDialog } from "@/hooks/use-modal-dialog";
import { DialogBackdrop } from "@/components/Overlay/DialogBackdrop";

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
  const dialogRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  useModalDialog({
    dialogRef,
    initialFocusRef: inputRef,
    onDismiss: onCancel,
    selectInitialText: true,
  });

  const label = confirmLabel || t("common.save");

  const dialogContent = (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <DialogBackdrop
        ariaLabel={t("common.close")}
        onDismiss={onCancel}
        className="absolute inset-0 bg-black/60"
      />

      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="input-dialog-title"
        tabIndex={-1}
        className="relative w-full max-w-sm rounded-lg border border-[var(--color-border)] bg-[var(--color-sidebar)] p-6 shadow-xl"
      >
        <h3
          id="input-dialog-title"
          className="break-words text-[15px] font-semibold text-[var(--color-text)]"
        >
          {title}
        </h3>

        <input
          ref={inputRef}
          type="text"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && value.trim()) {
              event.preventDefault();
              onConfirm(value.trim());
            }
          }}
          placeholder={placeholder}
          aria-label={title}
          className="mt-3 w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-3 py-2 text-[13px] text-[var(--color-text)] placeholder-[var(--color-text-dimmer)] outline-none transition-colors focus:border-[var(--color-accent)] focus:ring-1 focus:ring-[var(--color-accent)]/30"
        />

        <div className="mt-5 flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-4 py-2 text-[13px] text-[var(--color-text)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text)] focus:outline-none focus:ring-2 focus:ring-[var(--color-accent)]/30"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={() => value.trim() && onConfirm(value.trim())}
            disabled={!value.trim()}
            className="rounded-md bg-[var(--color-control-primary)] px-4 py-2 text-[13px] text-[var(--color-control-primary-foreground)] transition-colors hover:bg-[color-mix(in_srgb,var(--color-control-primary)_88%,white)] focus:outline-none focus:ring-2 focus:ring-[var(--color-focus-ring)] disabled:opacity-50"
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
