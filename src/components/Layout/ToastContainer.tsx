import { X, AlertCircle, CheckCircle, Info, AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";
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

function Toast({ notification }: { notification: Notification }) {
  const { t } = useTranslation();
  const dismiss = useNotificationStore((s) => s.dismissNotification);
  const Icon = ICON_MAP[notification.type];
  const iconColor =
    notification.type === "error"
      ? "text-[var(--color-destructive)]"
      : "text-[var(--color-text)]";

  return (
    <div className="animate-slide-up flex items-start gap-2.5 rounded-lg border border-[var(--color-border)] bg-[var(--color-sidebar)] px-3 py-2.5 shadow-lg">
      <Icon size={14} className={`mt-0.5 shrink-0 ${iconColor}`} />

      <div className="min-w-0 flex-1">
        <p className="break-words text-[12px] font-medium text-white">
          {notification.title}
        </p>
        {notification.message && (
          <p className="mt-0.5 break-words text-[11px] text-[var(--color-text-dim)]">
            {notification.message}
          </p>
        )}
        {notification.retryable && notification.retryAction && (
          <button
            onClick={() => {
              notification.retryAction?.();
              dismiss(notification.id);
            }}
            className="mt-1.5 rounded-sm text-[11px] text-[var(--color-accent)] hover:underline focus:outline-none focus:ring-2 focus:ring-[var(--color-accent)]/30"
          >
            {t("common.tryAgain")}
          </button>
        )}
      </div>

      <button
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
    <div className="fixed right-4 bottom-16 z-[100] flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-2">
      {notifications.map((n) => (
        <Toast key={n.id} notification={n} />
      ))}
    </div>
  );
}
