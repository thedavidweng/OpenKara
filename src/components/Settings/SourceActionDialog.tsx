import { useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useModalDialog } from "@/hooks/use-modal-dialog";
import { sourceActionButton } from "./source-market-utils";

interface SourceActionDialogProps {
  title: string;
  confirmLabel: string;
  busy?: boolean;
  confirmDisabled?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  children: ReactNode;
}

export function SourceActionDialog({
  title,
  confirmLabel,
  busy = false,
  confirmDisabled = false,
  onConfirm,
  onCancel,
  children,
}: SourceActionDialogProps) {
  const { t } = useTranslation();
  const dialog = useRef<HTMLDivElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  useModalDialog({
    dialogRef: dialog,
    initialFocusRef: cancel,
    canDismiss: !busy,
    onDismiss: onCancel,
  });
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        tabIndex={-1}
        aria-labelledby="source-action-dialog-title"
        className="max-w-lg space-y-4 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-6 text-[var(--color-text)]"
      >
        <h3 id="source-action-dialog-title" className="font-semibold">
          {title}
        </h3>
        <div className="space-y-3 text-sm">{children}</div>
        <div className="flex gap-2">
          <button
            ref={cancel}
            className={sourceActionButton}
            disabled={busy}
            onClick={onCancel}
          >
            {t("common.cancel")}
          </button>
          <button
            className={sourceActionButton}
            disabled={busy || confirmDisabled}
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
