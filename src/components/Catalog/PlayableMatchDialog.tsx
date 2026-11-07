import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { DialogBackdrop } from "@/components/Overlay/DialogBackdrop";
import { formatDuration } from "@/components/Catalog/streaming-utils";
import { useModalDialog } from "@/hooks/use-modal-dialog";
import { onlinePreviewQueueId } from "@/playback";
import { useCatalogStore } from "@/stores/catalog-store";
import type { StreamingTrack } from "@/types/ipc";

const focusRing =
  "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)]";

const trackKey = (track: StreamingTrack) =>
  onlinePreviewQueueId(track.source_id, track.remote_track_id);

function TrackDetails({
  track,
  sourceName,
}: {
  track: StreamingTrack;
  sourceName: string;
}) {
  const details = [
    track.artist,
    track.album,
    formatDuration(track.duration_ms),
    sourceName,
  ].filter(Boolean);
  return (
    <span className="min-w-0">
      <span className="block truncate text-[14px] font-semibold text-[var(--color-text)]">
        {track.title}
      </span>
      <span className="block truncate text-[12px] text-[var(--color-text-dim)]">
        {details.join(" · ")}
      </span>
    </span>
  );
}

export function PlayableMatchDialog() {
  const { t } = useTranslation();
  const match = useCatalogStore((s) => s.playableMatch);
  const sources = useCatalogStore((s) => s.sources);
  const confirm = useCatalogStore((s) => s.confirmPlayableMatch);
  const close = useCatalogStore((s) => s.closePlayableMatch);
  const dialogRef = useRef<HTMLDivElement>(null);
  const [chosen, setChosen] = useState<string | null>(null);

  useModalDialog({
    dialogRef,
    onDismiss: close,
    enabled: match !== null,
  });

  const status = match?.status;
  const candidates = match?.candidates ?? [];
  const selected =
    candidates.find((candidate) => trackKey(candidate) === chosen) ??
    candidates[0];

  useEffect(() => {
    setChosen(null);
    if (status !== "ready") return;
    const dialog = dialogRef.current;
    if (dialog?.contains(document.activeElement))
      dialog
        .querySelector<HTMLInputElement>("input[type=radio]:checked")
        ?.focus({ preventScroll: true });
  }, [status]);

  if (!match) return null;

  const sourceName = (track: StreamingTrack) =>
    sources.find((source) => source.source_id === track.source_id)?.name ??
    track.source_id;

  return (
    <div className="fixed inset-0 z-[80] flex items-center justify-center p-4">
      <DialogBackdrop
        ariaLabel={t("common.close")}
        onDismiss={close}
        className="absolute inset-0 bg-black/60"
      />
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="playable-match-title"
        tabIndex={-1}
        className="app-panel-surface relative w-full max-w-lg rounded-xl border border-[var(--color-border)] bg-[color-mix(in_srgb,var(--color-sidebar)_96%,transparent)] p-6"
      >
        <h3
          id="playable-match-title"
          className="text-[16px] font-semibold text-[var(--color-text)]"
        >
          {t("catalog.streaming.playableMatch.title")}
        </h3>
        <div className="mt-4">
          <div className="text-[12px] font-medium text-[var(--color-text-dim)]">
            {t("catalog.streaming.playableMatch.original")}
          </div>
          <div className="mt-1 flex">
            <TrackDetails
              track={match.original}
              sourceName={sourceName(match.original)}
            />
          </div>
        </div>
        {status === "loading" ? (
          <p
            role="status"
            className="mt-4 text-[13px] text-[var(--color-text-dim)]"
          >
            {t("catalog.streaming.playableMatch.loading")}
          </p>
        ) : candidates.length === 0 ? (
          <p
            role="status"
            className="mt-4 text-[13px] text-[var(--color-text-dim)]"
          >
            {t("catalog.streaming.playableMatch.empty")}
          </p>
        ) : (
          <fieldset className="mt-4 min-w-0">
            <legend className="text-[12px] font-medium text-[var(--color-text-dim)]">
              {t("catalog.streaming.playableMatch.versions")}
            </legend>
            <div className="mt-1 flex max-h-72 flex-col overflow-y-auto">
              {candidates.map((candidate) => {
                const key = trackKey(candidate);
                const sameIsrc =
                  !!candidate.isrc && candidate.isrc === match.original.isrc;
                return (
                  <label
                    key={key}
                    className="flex cursor-pointer items-center gap-3 rounded-md px-2 py-2 hover:bg-[var(--color-ghost-hover)]"
                  >
                    <input
                      type="radio"
                      name="playable-match"
                      checked={selected === candidate}
                      onChange={() => setChosen(key)}
                      className={`size-4 shrink-0 accent-[var(--color-accent)] ${focusRing}`}
                    />
                    <TrackDetails
                      track={candidate}
                      sourceName={sourceName(candidate)}
                    />
                    {sameIsrc ? (
                      <span className="ml-auto shrink-0 rounded-full border border-[var(--color-border-light)] px-2 py-0.5 text-[11px] text-[var(--color-text-dim)]">
                        {t("catalog.streaming.playableMatch.sameIsrc")}
                      </span>
                    ) : null}
                  </label>
                );
              })}
            </div>
          </fieldset>
        )}
        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={close}
            className={`rounded-md border border-[var(--color-border-light)] px-4 py-2 text-[13px] ${focusRing}`}
          >
            {t("common.cancel")}
          </button>
          <button
            type="button"
            disabled={!selected}
            onClick={() => selected && void confirm(selected)}
            className={`rounded-md bg-[var(--color-accent)] px-4 py-2 text-[13px] font-medium text-[var(--color-on-accent)] disabled:opacity-40 ${focusRing}`}
          >
            {t("catalog.streaming.playableMatch.import")}
          </button>
        </div>
      </div>
    </div>
  );
}
