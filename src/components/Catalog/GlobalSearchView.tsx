import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { Play } from "lucide-react";
import { formatDuration } from "@/components/Catalog/streaming-utils";
import { songDisplayTitle } from "@/lib/song-display";
import { useCatalogStore } from "@/stores/catalog-store";
import { useLibraryStore } from "@/stores/library-store";
import { usePlayerStore } from "@/stores/player-store";
import { StreamingTrackList } from "./StreamingTrackList";

/** Library matches shown here; the sidebar list holds all of them. */
const LIBRARY_RESULT_LIMIT = 8;

const button =
  "min-h-9 rounded-full border border-[var(--color-border)] px-4 text-[13px] focus-visible:outline-2 focus-visible:outline-[var(--color-focus-ring)] disabled:opacity-50";

export function GlobalSearchView() {
  const { t } = useTranslation();
  const scroll = useRef<HTMLDivElement>(null);
  const { query, sections } = useCatalogStore((s) => s.globalSearch);
  const sources = useCatalogStore((s) => s.sources);
  const searchMoreIn = useCatalogStore((s) => s.searchMoreIn);
  const setActiveSource = useCatalogStore((s) => s.setActiveSource);
  const libraryQuery = useLibraryStore((s) => s.searchQuery.trim());
  const songsQuery = useLibraryStore((s) => s.songsQuery);
  const songs = useLibraryStore((s) => s.songs);
  const playSong = usePlayerStore((s) => s.playSong);
  const libraryPending = !!libraryQuery && songsQuery !== libraryQuery;
  const librarySongs = libraryQuery && !libraryPending ? songs : [];

  return (
    <section
      data-testid="global-search"
      aria-label={t("globalSearch.regionLabel")}
      className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-[var(--color-surface)]"
    >
      <div
        ref={scroll}
        className="min-h-0 flex-1 overflow-auto px-6 pb-24 pt-5"
      >
        <h1 className="mb-4 text-xl font-bold">
          {t("globalSearch.title", { query })}
        </h1>

        <section aria-labelledby="global-search-library" className="mb-6">
          <h2
            id="global-search-library"
            className="mb-2 text-[15px] font-semibold"
          >
            {t("globalSearch.library")}
          </h2>
          {libraryPending ? (
            <p
              role="status"
              className="text-[13px] text-[var(--color-text-dim)]"
            >
              {t("globalSearch.searching")}
            </p>
          ) : librarySongs.length === 0 ? (
            <p className="text-[13px] text-[var(--color-text-dim)]">
              {t("globalSearch.libraryEmpty")}
            </p>
          ) : (
            <>
              <ul className="mx-4">
                {librarySongs.slice(0, LIBRARY_RESULT_LIMIT).map((song) => {
                  const title = songDisplayTitle(song);
                  return (
                    <li key={song.hash}>
                      <button
                        type="button"
                        onClick={() => playSong(song.hash)}
                        className="group flex min-h-11 w-full items-center gap-3 rounded-lg px-2 text-left hover:bg-[var(--color-ghost-hover)] focus-visible:outline-2 focus-visible:outline-[var(--color-focus-ring)]"
                      >
                        <Play
                          size={14}
                          aria-hidden="true"
                          className="shrink-0 text-[var(--color-text-dim)] group-hover:text-[var(--color-text)]"
                        />
                        <span className="min-w-0 flex-1">
                          <span className="block truncate text-[13px]">
                            {title}
                          </span>
                          {song.artist ? (
                            <span className="block truncate text-[12px] text-[var(--color-text-dim)]">
                              {song.artist}
                            </span>
                          ) : null}
                        </span>
                        <span className="shrink-0 text-[12px] tabular-nums text-[var(--color-text-dim)]">
                          {formatDuration(song.duration_ms)}
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ul>
              {librarySongs.length > LIBRARY_RESULT_LIMIT ? (
                <p className="mx-4 mt-2 px-2 text-[12px] text-[var(--color-text-dim)]">
                  {t("globalSearch.libraryMore", {
                    shown: LIBRARY_RESULT_LIMIT,
                    count: librarySongs.length,
                  })}
                </p>
              ) : null}
            </>
          )}
        </section>

        {Object.entries(sections).map(([id, section]) => {
          const name =
            sources.find((source) => source.source_id === id)?.name ?? id;
          const headingId = `global-search-${id}`;
          return (
            <section key={id} aria-labelledby={headingId} className="mb-6">
              <h2 id={headingId} className="mb-2 text-[15px] font-semibold">
                {name}
              </h2>
              {section.status === "signed_out" ? (
                <div className="flex flex-wrap items-center gap-3 text-[13px]">
                  <p>{t("globalSearch.signInToSearch", { name })}</p>
                  <button
                    type="button"
                    className={button}
                    onClick={() => setActiveSource(id)}
                  >
                    {t("globalSearch.openSource", { name })}
                  </button>
                </div>
              ) : section.status === "error" ? (
                <div
                  role="alert"
                  className="flex flex-wrap items-center gap-3 text-[13px]"
                >
                  <p>{t("globalSearch.searchFailed", { name })}</p>
                  <button
                    type="button"
                    className={button}
                    onClick={() => void searchMoreIn(id)}
                  >
                    {t("common.retry")}
                  </button>
                </div>
              ) : section.status === "loading" &&
                section.tracks.length === 0 ? (
                <p
                  role="status"
                  className="text-[13px] text-[var(--color-text-dim)]"
                >
                  {t("globalSearch.searching")}
                </p>
              ) : section.status === "ready" && section.tracks.length === 0 ? (
                <p className="text-[13px] text-[var(--color-text-dim)]">
                  {t("catalog.streaming.resultsEmpty", { query })}
                </p>
              ) : null}
              {section.tracks.length > 0 ? (
                <StreamingTrackList
                  tracks={section.tracks}
                  scrollRef={scroll}
                  loadingMore={section.status === "loading"}
                />
              ) : null}
              {section.cursor && section.status !== "error" ? (
                <button
                  type="button"
                  className={`${button} mt-2`}
                  disabled={section.status === "loading"}
                  onClick={() => void searchMoreIn(id)}
                >
                  {t("catalog.streaming.loadMore")}
                </button>
              ) : null}
            </section>
          );
        })}
      </div>
    </section>
  );
}
