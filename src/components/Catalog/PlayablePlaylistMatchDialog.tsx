import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { DialogBackdrop } from "@/components/Overlay/DialogBackdrop";
import { formatDuration } from "@/components/Catalog/streaming-utils";
import { useModalDialog } from "@/hooks/use-modal-dialog";
import { onlinePreviewQueueId } from "@/playback";
import { usePlaylistStore } from "@/stores/playlist-store";
import { useCatalogStore } from "@/stores/catalog-store";
import type { StreamingTrack } from "@/types/ipc";

const focusRing =
  "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)]";

const trackKey = (track: StreamingTrack) =>
  onlinePreviewQueueId(track.source_id, track.remote_track_id);

export function PlayablePlaylistMatchDialog() {
  const { t } = useTranslation();
  const match = useCatalogStore((s) => s.playlistMatch);
  const sources = useCatalogStore((s) => s.sources);
  const run = useCatalogStore((s) => s.importRun);
  const detail = useCatalogStore(
    (s) => s.sourceStates[s.playlistMatch?.sourceId ?? ""]?.playlistDetail,
  );
  const setChoice = useCatalogStore((s) => s.setPlaylistMatchChoice);
  const setAdd = useCatalogStore((s) => s.setPlaylistMatchAddToPlaylist);
  const confirm = useCatalogStore((s) => s.confirmPlaylistMatches);
  const cancelImport = useCatalogStore((s) => s.cancelPlaylistMatchImport);
  const close = useCatalogStore((s) => s.closePlaylistMatch);
  const dialogRef = useRef<HTMLDivElement>(null);
  const phase = match?.phase;
  const playlistName = match?.playlistName;

  useModalDialog({
    dialogRef,
    onDismiss: close,
    canDismiss: phase !== "importing",
    enabled: match !== null,
  });

  useEffect(() => {
    if (phase === "done" && playlistName)
      void usePlaylistStore.getState().loadPlaylists();
  }, [phase, playlistName]);

  useEffect(() => {
    const dialog = dialogRef.current;
    const active = document.activeElement;
    if (
      !phase ||
      !dialog ||
      !(active === document.body || dialog.contains(active))
    )
      return;
    dialog.querySelector<HTMLElement>("select, button")?.focus({
      preventScroll: true,
    });
  }, [phase]);

  if (!match) return null;

  const sourceName = (id: string) =>
    sources.find((source) => source.source_id === id)?.name ?? id;
  const optionLabel = (track: StreamingTrack, remembered: boolean) => {
    const label = [
      `${track.title} — ${track.artist}`,
      formatDuration(track.duration_ms),
      sourceName(track.source_id),
    ]
      .filter(Boolean)
      .join(" · ");
    return remembered
      ? t("catalog.streaming.playlistMatch.remembered", { option: label })
      : label;
  };
  const total = match.rows.length;
  const selected = match.rows.filter((row) => row.choice).length;
  const current = Math.min(
    match.finishedBefore + (run?.completed ?? 0) + 1,
    match.importTotal,
  );

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
        aria-labelledby="playlist-match-title"
        tabIndex={-1}
        className="app-panel-surface relative flex max-h-[85vh] w-full max-w-2xl flex-col rounded-xl border border-[var(--color-border)] bg-[color-mix(in_srgb,var(--color-sidebar)_96%,transparent)] p-6"
      >
        <h3
          id="playlist-match-title"
          className="text-[16px] font-semibold text-[var(--color-text)]"
        >
          {t("catalog.streaming.playlistMatch.title")}
        </h3>
        <p className="mt-1 text-[12px] text-[var(--color-text-dim)]">
          {match.name}
          {detail?.next_cursor
            ? ` · ${t("catalog.streaming.playlistMatch.loaded", { count: detail.tracks.length })}`
            : ""}
        </p>
        {phase === "searching" ? (
          <p
            role="status"
            className="mt-4 text-[13px] text-[var(--color-text-dim)]"
          >
            {t("catalog.streaming.playlistMatch.searching", {
              current: Math.min(match.searched + 1, match.searchTotal),
              total: match.searchTotal,
            })}
          </p>
        ) : null}
        {phase === "review" ? (
          <ul className="mt-4 min-h-0 flex-1 divide-y divide-[var(--color-border)] overflow-y-auto">
            {match.rows.map((row, index) => (
              <li key={trackKey(row.original)} className="py-2">
                <div className="truncate text-[14px] font-semibold text-[var(--color-text)]">
                  {row.original.title}
                </div>
                <div className="truncate text-[12px] text-[var(--color-text-dim)]">
                  {[
                    row.original.artist,
                    formatDuration(row.original.duration_ms),
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </div>
                {row.options.length === 0 ? (
                  <p className="mt-1 text-[12px] text-[var(--color-text-dim)]">
                    {t("catalog.streaming.playlistMatch.noCandidate")}
                  </p>
                ) : (
                  <select
                    aria-label={t("catalog.streaming.playlistMatch.select", {
                      title: row.original.title,
                    })}
                    value={row.choice ?? ""}
                    onChange={(event) =>
                      setChoice(index, event.target.value || null)
                    }
                    className={`mt-1 w-full rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface-muted)] px-2 py-1.5 text-[13px] text-[var(--color-text)] ${focusRing}`}
                  >
                    {row.options.map((option) => (
                      <option key={trackKey(option)} value={trackKey(option)}>
                        {optionLabel(
                          option,
                          trackKey(option) === row.rememberedKey,
                        )}
                      </option>
                    ))}
                    <option value="">
                      {t("catalog.streaming.playlistMatch.skip")}
                    </option>
                  </select>
                )}
              </li>
            ))}
          </ul>
        ) : null}
        {phase === "review" ? (
          <div className="mt-4 space-y-2">
            <p
              role="status"
              className="text-[13px] text-[var(--color-text-dim)]"
            >
              {t("catalog.streaming.playlistMatch.summary", {
                selected,
                total,
              })}
            </p>
            <label className="flex items-center gap-2 text-[13px] text-[var(--color-text)]">
              <input
                type="checkbox"
                checked={match.addToPlaylist}
                onChange={(event) => setAdd(event.target.checked)}
                className={`size-4 accent-[var(--color-accent)] ${focusRing}`}
              />
              {t("catalog.streaming.playlistMatch.addToPlaylist")}
            </label>
          </div>
        ) : null}
        {phase === "importing" ? (
          <p
            role="status"
            className="mt-4 text-[13px] text-[var(--color-text-dim)]"
          >
            {match.cancelling
              ? t("catalog.streaming.cancelling")
              : t("catalog.streaming.playlistMatch.importing", {
                  current,
                  total: match.importTotal,
                })}
          </p>
        ) : null}
        {phase === "done" ? (
          <div className="mt-4 min-h-0 flex-1 overflow-y-auto">
            <p role="status" className="text-[13px] text-[var(--color-text)]">
              {t("catalog.streaming.playlistMatch.imported", {
                imported: match.imported,
              })}
              {match.playlistName
                ? ` ${t("catalog.streaming.playlistMatch.addedTo", { name: match.playlistName })}`
                : ""}
            </p>
            {match.failures.length > 0 ? (
              <section aria-labelledby="playlist-match-failed" className="mt-3">
                <h4
                  id="playlist-match-failed"
                  className="text-[12px] font-medium text-[var(--color-text-dim)]"
                >
                  {t("catalog.streaming.playlistMatch.notImported", {
                    count: match.failures.length,
                  })}
                </h4>
                <ul className="mt-1 space-y-1 text-[12px] text-[var(--color-text-dim)]">
                  {match.failures.map((failure, index) => (
                    <li key={`${failure.title}-${index}`}>
                      <span className="text-[var(--color-text)]">
                        {failure.title}
                      </span>
                      {failure.artist ? ` — ${failure.artist}` : ""} ·{" "}
                      {failure.refusal
                        ? t(
                            `catalog.streaming.refusalReason.${failure.refusal.reason}`,
                          )
                        : failure.reason === "noCandidate" ||
                            failure.reason === "skipped"
                          ? t(
                              `catalog.streaming.playlistMatch.${failure.reason}`,
                            )
                          : t(`catalog.streaming.skipReason.${failure.reason}`)}
                    </li>
                  ))}
                </ul>
              </section>
            ) : null}
          </div>
        ) : null}
        <div className="mt-5 flex justify-end gap-2">
          {phase === "importing" ? (
            <button
              type="button"
              disabled={match.cancelling}
              onClick={() => void cancelImport()}
              className={`rounded-md border border-[var(--color-border-light)] px-4 py-2 text-[13px] disabled:opacity-40 ${focusRing}`}
            >
              {t("catalog.streaming.cancelImport")}
            </button>
          ) : (
            <button
              type="button"
              onClick={close}
              className={`rounded-md border border-[var(--color-border-light)] px-4 py-2 text-[13px] ${focusRing}`}
            >
              {phase === "done" ? t("common.close") : t("common.cancel")}
            </button>
          )}
          {phase === "review" ? (
            <button
              type="button"
              disabled={selected === 0}
              onClick={() => void confirm()}
              className={`rounded-md bg-[var(--color-accent)] px-4 py-2 text-[13px] font-medium text-[var(--color-on-accent)] disabled:opacity-40 ${focusRing}`}
            >
              {t("catalog.streaming.playlistMatch.import")}
            </button>
          ) : null}
        </div>
      </div>
    </div>
  );
}
