import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronUp, LoaderCircle } from "lucide-react";
import { Tooltip } from "@/components/Overlay/Tooltip";
import type { PlainTextPageDirection } from "@/lib/plain-text-page-controls";
import type { RemotePageModel } from "./lyrics-panel-model";

const BUTTON_CLASS =
  "motion-icon-button rounded-full border border-[var(--color-border-light)] bg-[var(--color-sidebar)] p-2 text-[var(--color-text-dim)] hover:border-[color-mix(in_srgb,var(--color-accent)_28%,var(--color-border-light))] hover:bg-[var(--color-hover)] hover:text-[var(--color-control-primary)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-accent)]/50";

/**
 * Pages plain-text lyrics on the audience display. The local view never
 * scrolls in response — only the remote surface moves — so the buttons stay
 * disabled until AirPlay confirms the page it is showing.
 */
export function RemotePlainTextPageControls({
  remotePage,
}: {
  remotePage: RemotePageModel;
}) {
  const { t } = useTranslation();

  const renderButton = (
    direction: PlainTextPageDirection,
    label: string,
    Icon: typeof ChevronUp,
  ) => {
    const awaitingRemote =
      remotePage.locked && remotePage.pendingDirection === direction;

    return (
      <Tooltip label={label}>
        <button
          type="button"
          data-testid={`plain-text-page-${direction}`}
          data-airplay-page-pending={awaitingRemote ? "true" : "false"}
          onClick={() => remotePage.step(direction)}
          aria-label={label}
          disabled={remotePage.locked}
          className={BUTTON_CLASS}
        >
          {awaitingRemote ? (
            <LoaderCircle size={16} className="animate-spin" />
          ) : (
            <Icon size={16} />
          )}
        </button>
      </Tooltip>
    );
  };

  return (
    <div className="pointer-events-none absolute inset-y-0 right-4 z-10 flex items-center">
      <div className="pointer-events-auto flex flex-col gap-3">
        {renderButton("prev", t("lyrics.previousPage"), ChevronUp)}
        {renderButton("next", t("lyrics.nextPage"), ChevronDown)}
      </div>
    </div>
  );
}
