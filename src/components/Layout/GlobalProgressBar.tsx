import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import { useActiveTasks } from "@/hooks/use-active-tasks";
import type { ActiveTask } from "@/lib/task-progress";

interface TaskProgressBarProps extends Omit<ActiveTask, "key"> {
  className?: string;
  compact?: boolean;
  ariaLabel?: string;
}

export function TaskProgressBar({
  label,
  detail,
  percent,
  indeterminate,
  onCancel,
  className,
  compact = false,
  ariaLabel,
}: TaskProgressBarProps) {
  const { t } = useTranslation();
  const clampedPercent = Math.min(100, Math.max(0, percent));

  if (compact) {
    return (
      <div
        className={className ?? "w-full"}
        role="progressbar"
        aria-label={ariaLabel || label || undefined}
        aria-valuenow={indeterminate ? undefined : clampedPercent}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <div className="h-1 w-full overflow-hidden rounded-full bg-[var(--color-border)]">
          {indeterminate ? (
            <div className="relative h-full w-full overflow-hidden rounded-full bg-[color-mix(in_srgb,var(--color-accent)_22%,transparent)]">
              <div className="model-indeterminate-bar absolute inset-y-0 left-0 rounded-full bg-[var(--color-accent)] will-change-transform" />
            </div>
          ) : (
            <div
              className="motion-surface h-full rounded-full bg-[var(--color-accent)]"
              style={{ width: `${clampedPercent}%` }}
            />
          )}
        </div>
      </div>
    );
  }

  return (
    <div className={className ?? "space-y-1"}>
      <div className="flex items-center justify-between">
        <span className="min-w-0 truncate text-[11px] text-[var(--color-text-dim)]">
          {label}
          {detail && (
            <span className="ml-1 text-[var(--color-text-dimmer)]">
              {detail}
            </span>
          )}
        </span>
        {onCancel && (
          <button
            type="button"
            onClick={onCancel}
            aria-label={t("common.cancel")}
            className="motion-icon-button shrink-0 rounded p-0.5 text-[var(--color-text-dim)] hover:bg-[var(--color-ghost-hover)] hover:text-[var(--color-text)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50"
          >
            <X size={12} />
          </button>
        )}
      </div>
      <div
        className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--color-border)]"
        role="progressbar"
        aria-label={ariaLabel || label || undefined}
        aria-valuenow={indeterminate ? undefined : clampedPercent}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        {indeterminate ? (
          <div className="relative h-full w-full overflow-hidden rounded-full bg-[color-mix(in_srgb,var(--color-accent)_22%,transparent)]">
            <div className="model-indeterminate-bar absolute inset-y-0 left-0 rounded-full bg-[var(--color-accent)] will-change-transform" />
          </div>
        ) : (
          <div
            className="motion-surface h-full rounded-full bg-[var(--color-accent)]"
            style={{ width: `${clampedPercent}%` }}
          />
        )}
      </div>
    </div>
  );
}

export function GlobalProgressBar() {
  const tasks = useActiveTasks();

  if (tasks.length === 0) return null;

  return (
    <div className="z-10 space-y-2 bg-[var(--color-sidebar)] px-3 py-2">
      {tasks.map(({ key, ...task }) => (
        <TaskProgressBar key={key} {...task} />
      ))}
    </div>
  );
}
