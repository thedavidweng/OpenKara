import { useState, useRef, useEffect } from "react";
import { X, AlertCircle, CheckCircle, Info, AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { copyDebugInfo } from "@/lib/debug-info";
import { notifyError } from "@/lib/errors";
import {
  useNotificationStore,
  type Notification,
} from "@/stores/notification-store";

const ICON_MAP = {
  error: AlertCircle,
  warning: AlertTriangle,
  success: CheckCircle,
  info: Info,
} as const;

const ACTION_BUTTON_CLASS =
  "rounded-sm text-[11px] text-[var(--color-accent)] hover:underline focus:outline-none focus:ring-2 focus:ring-[var(--color-accent)]/30";

const COPIED_RESET_MS = 2000;

function Toast({ notification }: { notification: Notification }) {
  const { t } = useTranslation();
  const dismiss = useNotificationStore((s) => s.dismissNotification);
  const [debugCopied, setDebugCopied] = useState(false);
  const copiedTimeoutRef = useRef<number | undefined>();
  const Icon = ICON_MAP[notification.type];
  const iconColor =
    notification.type === "error"
      ? "text-[var(--color-destructive)]"
      : "text-[var(--color-text)]";
  const isAssertive =
    notification.type === "error" || notification.type === "warning";
  const showRetry = notification.retryable && Boolean(notification.retryAction);
  const showCopyDebug = notification.type === "error";
  const showActions = showRetry || showCopyDebug;

  useEffect(() => {
    return () => {
      if (copiedTimeoutRef.current !== undefined) {
        window.clearTimeout(copiedTimeoutRef.current);
      }
    };
  }, []);

  const handleCopyDebug = async () => {
    try {
      await copyDebugInfo({ translate: t });
      setDebugCopied(true);
      if (copiedTimeoutRef.current !== undefined) {
        window.clearTimeout(copiedTimeoutRef.current);
      }
      copiedTimeoutRef.current = window.setTimeout(() => setDebugCopied(false), COPIED_RESET_MS);
    } catch (error) {
      notifyError(error);
    }
  };

  return (
    <div
      role={isAssertive ? "alert" : "status"}
      aria-live={isAssertive ? "assertive" : "polite"}
      aria-atomic="true"
      className="animate-slide-up flex items-start gap-2.5 rounded-lg border border-[var(--color-border)] bg-[var(--color-sidebar)] px-3 py-2.5 shadow-lg"
    >
      <Icon size={14} className={`mt-0.5 shrink-0 ${iconColor}`} />

      <div className="min-w-0 flex-1">
        <p className="break-words text-[12px] font-medium text-[var(--color-text)]">
          {notification.title}
        </p>
        {notification.message && (
          <p className="mt-0.5 break-words whitespace-pre-line text-[11px] text-[var(--color-text-dim)]">
            {notification.message}
          </p>
        )}
        {showActions && (
          <div className="mt-1.5 flex flex-wrap gap-x-3 gap-y-1">
            {showRetry && (
              <button
                type="button"
                onClick={() => {
                  notification.retryAction?.();
                  dismiss(notification.id);
                }}
                className={ACTION_BUTTON_CLASS}
              >
                {t("common.tryAgain")}
              </button>
            )}
            {showCopyDebug && (
              <button
                type="button"
                onClick={() => void handleCopyDebug()}
                className={ACTION_BUTTON_CLASS}
              >
                {debugCopied
                  ? t("settings.about.copied")
                  : t("settings.about.copyDebugInfo")}
              </button>
            )}
          </div>
        )}
      </div>

      <button
        type="button"
        onClick={() => dismiss(notification.id)}
        aria-label={t("common.close")}
        className="shrink-0 rounded-sm text-[var(--color-text-dimmer)] hover:text-[var(--color-text-dim)] focus:outline-none focus:ring-2 focus:ring-[var(--color-accent)]/30"
      >
        <X size={12} />
      </button>
    </div>
  );
}

export function ToastContainer() {
  const notifications = useNotificationStore((s) => s.notifications);

  if (notifications.length === 0) return null;

  return (
    <div className="fixed right-4 bottom-24 z-[100] flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-2">
      {notifications.map((n) => (
        <Toast key={n.id} notification={n} />
      ))}
    </div>
  );
}
