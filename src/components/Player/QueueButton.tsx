import { ListMusic } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Tooltip } from "@/components/Overlay/Tooltip";
import { useQueueStore } from "@/stores/queue-store";

export function QueueButton() {
  const { t } = useTranslation();
  const queue = useQueueStore((s) => s.queue);
  const togglePanel = useQueueStore((s) => s.togglePanel);
  const isOpen = useQueueStore((s) => s.isOpen);

  return (
    <Tooltip label={t("queue.title")}>
      <button
        id="queue-button"
        onClick={togglePanel}
        aria-label={t("queue.title")}
        aria-pressed={isOpen}
        data-playback-action="queue"
        data-active={isOpen ? "true" : undefined}
        className={`motion-icon-button playback-bar-action-button relative ${
          isOpen
            ? "text-[var(--color-accent)]"
            : "text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
        }`}
        data-queue-button-visual-variant="unified"
      >
        <ListMusic size={18} />
        {queue.length > 0 && (
          <span
            aria-hidden="true"
            className="absolute -right-1.5 -top-1.5 flex h-3.5 w-3.5 items-center justify-center rounded-full bg-[var(--color-accent)] text-[8px] font-bold text-[var(--color-on-accent)]"
          >
            {queue.length > 9 ? "9+" : queue.length}
          </span>
        )}
      </button>
    </Tooltip>
  );
}
