import { useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useModalDialog } from "@/hooks/use-modal-dialog";
import { sourceActionButton, sourcePrimaryButton } from "./source-market-utils";

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
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--color-scrim)] p-4">
      <div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        tabIndex={-1}
        aria-labelledby="source-action-dialog-title"
        className="w-full max-w-md space-y-4 rounded-xl bg-popover p-5 text-popover-foreground shadow-[var(--shadow-dialog)] ring-1 ring-foreground/10 outline-none"
      >
        <h3
          id="source-action-dialog-title"
          className="text-[15px] font-semibold"
        >
          {title}
        </h3>
        <div className="space-y-3 text-muted-foreground [&_strong]:text-foreground">
          {children}
        </div>
        <div className="flex justify-end gap-2 pt-1">
          <button
            ref={cancel}
            className={sourceActionButton}
            disabled={busy}
            onClick={onCancel}
          >
            {t("common.cancel")}
          </button>
          <button
            className={sourcePrimaryButton}
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
