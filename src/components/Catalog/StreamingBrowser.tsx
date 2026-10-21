import { useEffect, useRef, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { APP_SHORTCUTS, matchesShortcut } from "@/lib/app-shortcuts";
import {
  sourceAllows,
  useCatalogStore,
  type StreamingTab,
} from "@/stores/catalog-store";
import { StreamingAccountMenu } from "./StreamingAccountMenu";
import { formatUpdated } from "./streaming-utils";
import { StreamingCover } from "./StreamingCover";
import { StreamingSignIn } from "./StreamingSignIn";
import { StreamingSignInErrorBanner } from "./StreamingSignInErrorBanner";
import { StreamingTrackList } from "./StreamingTrackList";

const button =
  "min-h-9 rounded-full border border-[var(--color-border)] px-4 text-[13px] focus-visible:outline-2 focus-visible:outline-[var(--color-focus-ring)] disabled:opacity-50";
export function StreamingBrowser() {
  const { t, i18n } = useTranslation();
  const state = useCatalogStore((value) => value);
  const source = state.sources.find(
    (item) => item.source_id === state.activeSourceId,
  );
  const capabilities = source?.manifest?.capabilities ?? [];
  const supportsSearch = capabilities.includes("search");
  const tabs: StreamingTab[] = [];
  if (capabilities.includes("liked_tracks")) tabs.push("liked");
  if (
    capabilities.includes("playlists") &&
    capabilities.includes("playlist_tracks")
  )
    tabs.push("playlists");
  if (
    capabilities.includes("search") &&
    (state.searchQuery || tabs.length === 0)
  )
    tabs.push("results");
  const active = tabs.includes(state.browserTab) ? state.browserTab : tabs[0];
  const operation =
    active === "liked"
      ? "liked_tracks"
      : active === "playlists"
        ? state.selectedPlaylistId
          ? "playlist_tracks"
          : "playlists"
        : "search";
  const available = sourceAllows(source, operation, state.session);
  const ready =
    !!source?.installed_version &&
    source.enabled &&
    source.runtime_ready &&
    ![
      "not_installed",
      "disabled",
      "update_required",
      "unavailable",
      "cancelling",
    ].includes(source.state);
  const draft = state.searchDraft;
  const setDraft = state.setSearchDraft;
  const input = useRef<HTMLInputElement>(null);
  const scroll = useRef<HTMLDivElement>(null);
  const tabRefs = useRef<
    Partial<Record<StreamingTab, HTMLButtonElement | null>>
  >({});
  const {
    activeSourceId,
    likedStatus,
    playlistsStatus,
    sessionStatus,
    loadLiked,
    loadPlaylists,
    loadSession,
  } = state;
  useEffect(() => {
    if (!ready || !available) return;
    if (active === "liked" && likedStatus === "idle") void loadLiked();
    if (active === "playlists" && playlistsStatus === "idle")
      void loadPlaylists();
  }, [
    ready,
    available,
    active,
    activeSourceId,
    likedStatus,
    playlistsStatus,
    loadLiked,
    loadPlaylists,
  ]);
  useEffect(() => {
    if (!ready || sessionStatus !== "idle") return;
    void loadSession();
  }, [ready, activeSourceId, sessionStatus, loadSession]);
  useEffect(() => {
    const handler = (event: globalThis.KeyboardEvent) => {
      if (supportsSearch && matchesShortcut(APP_SHORTCUTS.findInView, event)) {
        event.preventDefault();
        input.current?.focus();
        input.current?.select();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [supportsSearch]);
  const tabKey = (event: KeyboardEvent, tab: StreamingTab) => {
    let index = tabs.indexOf(tab);
    if (event.key === "ArrowRight") index = (index + 1) % tabs.length;
    else if (event.key === "ArrowLeft")
      index = (index + tabs.length - 1) % tabs.length;
    else if (event.key === "Home") index = 0;
    else if (event.key === "End") index = tabs.length - 1;
    else return;
    event.preventDefault();
    state.setBrowserTab(tabs[index]);
    tabRefs.current[tabs[index]]?.focus();
  };
  const detail = state.playlistDetail;
  const isDetail = active === "playlists" && !!state.selectedPlaylistId;
  const tracks = isDetail
    ? (detail?.tracks ?? [])
    : active === "liked"
      ? state.liked
      : state.searchResults;
  const status = isDetail
    ? state.playlistDetailStatus
    : active === "liked"
      ? state.likedStatus
      : active === "playlists"
        ? state.playlistsStatus
        : state.searchStatus;
  const cursor = isDetail
    ? detail?.next_cursor
    : active === "liked"
      ? state.likedCursor
      : active === "playlists"
        ? state.playlistsCursor
        : state.searchCursor;
  const load = (more = false) => {
    if (isDetail && state.selectedPlaylistId)
      void state.openPlaylist(state.selectedPlaylistId, more);
    else if (active === "liked") void state.loadLiked(more);
    else if (active === "playlists") void state.loadPlaylists(more);
    else void state.search(state.searchQuery || draft, more);
  };
  const canImport = sourceAllows(source, "import", state.session);
  const authDeclared = source?.manifest?.authentication;
  const hasAuth =
    !!authDeclared &&
    typeof authDeclared === "object" &&
    "flow" in authDeclared &&
    authDeclared.flow !== "none";
  return (
    <section
      data-testid="streaming-browser"
      aria-label={source?.name ?? t("catalog.streaming.unavailable")}
      className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-[var(--color-surface)]"
    >
      <div
        ref={scroll}
        className="min-h-0 flex-1 overflow-auto px-6 pb-24 pt-5"
      >
        <header className="mb-4 flex flex-wrap items-center gap-4">
          <h1 className="text-xl font-bold">
            {source?.name ??
              state.activeSourceId ??
              t("catalog.streaming.unavailable")}
          </h1>
          {state.session?.signed_in && !state.session.expired ? (
            <StreamingAccountMenu
              key={state.activeSourceId ?? ""}
              name={state.session.display_name ?? ""}
            />
          ) : hasAuth && ready ? (
            <StreamingSignIn key={state.activeSourceId ?? ""} compact />
          ) : null}
        </header>
        <StreamingSignInErrorBanner />
        {state.sessionStatus === "error" ? (
          <div role="alert">
            <p>{state.errors.session}</p>
            <button className={button} onClick={() => void state.loadSession()}>
              {t("common.retry")}
            </button>
          </div>
        ) : null}
        {!ready ? (
          <p role="status">{t("catalog.streaming.unavailable")}</p>
        ) : (
          <>
            {capabilities.includes("search") ? (
              <form
                role="search"
                className="mb-4 flex gap-2"
                onSubmit={(event) => {
                  event.preventDefault();
                  void state.search(draft);
                }}
              >
                <input
                  ref={input}
                  aria-label={t("catalog.streaming.searchPlaceholder")}
                  placeholder={t("catalog.streaming.searchPlaceholder")}
                  value={draft}
                  onChange={(event) => setDraft(event.target.value)}
                  className={`${button} min-w-0 flex-1 bg-[var(--color-surface-muted)]`}
                />
                <button
                  className={button}
                  type="submit"
                  disabled={!sourceAllows(source, "search", state.session)}
                >
                  {t("catalog.streaming.search")}
                </button>
                {draft ? (
                  <button
                    className={button}
                    type="button"
                    onClick={() => {
                      setDraft("");
                      state.clearSearch();
                      input.current?.focus();
                    }}
                  >
                    {t("catalog.streaming.clearSearch")}
                  </button>
                ) : null}
              </form>
            ) : null}
            <div
              role="tablist"
              aria-label={t("catalog.streaming.tabsLabel")}
              className="mb-4 flex gap-4"
            >
              {tabs.map((tab) => (
                <button
                  key={tab}
                  ref={(element) => {
                    tabRefs.current[tab] = element;
                  }}
                  role="tab"
                  id={`streaming-tab-${tab}`}
                  aria-controls="streaming-panel"
                  aria-selected={active === tab}
                  tabIndex={active === tab ? 0 : -1}
                  onClick={() => state.setBrowserTab(tab)}
                  onKeyDown={(event) => tabKey(event, tab)}
                  className={`${button} ${active === tab ? "font-bold" : "text-[var(--color-text-dim)]"}`}
                >
                  {t(
                    `catalog.streaming.${tab === "results" ? "results" : tab}`,
                  )}
                </button>
              ))}
            </div>
            {active ? (
              <div
                role="tabpanel"
                id="streaming-panel"
                aria-labelledby={`streaming-tab-${active}`}
              >
                {isDetail ? (
                  <button className={button} onClick={state.closePlaylist}>
                    {t("catalog.streaming.back")}
                  </button>
                ) : null}
                {!available ? (
                  <StreamingSignIn />
                ) : (
                  <>
                    {isDetail ? (
                      <div className="mb-4 flex items-center gap-4">
                        <h2 className="text-lg font-semibold">
                          {detail?.name}
                        </h2>
                        {detail?.creator ? (
                          <span>
                            {t("catalog.streaming.byCreator", {
                              creator: detail.creator,
                            })}
                          </span>
                        ) : null}
                        {detail?.track_count !== null &&
                        detail?.track_count !== undefined ? (
                          <span>
                            {t("catalog.streaming.songCount", {
                              count: detail.track_count,
                            })}
                          </span>
                        ) : null}
                        {detail?.updated_at_ms ? (
                          <span>
                            {t("catalog.streaming.updatedOn", {
                              date: formatUpdated(
                                detail.updated_at_ms,
                                i18n.language,
                              ),
                            })}
                          </span>
                        ) : null}
                        {detail?.description ? (
                          <p>{detail.description}</p>
                        ) : null}
                      </div>
                    ) : null}
                    {status === "error" ? (
                      <div role="alert" className="my-4">
                        <p>{t("catalog.streaming.loadFailed")}</p>
                        <p className="text-sm">
                          {
                            state.errors[
                              isDetail
                                ? "playlist"
                                : active === "liked"
                                  ? "liked"
                                  : active === "playlists"
                                    ? state.selectedPlaylistId
                                      ? "playlist_tracks"
                                      : "playlists"
                                    : "search"
                            ]
                          }
                        </p>
                        <button className={button} onClick={() => load()}>
                          {t("common.retry")}
                        </button>
                      </div>
                    ) : null}
                    {status === "loading" ? (
                      <p role="status">{t("common.loading")}</p>
                    ) : null}
                    {active === "playlists" && !isDetail ? (
                      <div className="grid grid-cols-[repeat(auto-fill,minmax(140px,1fr))] gap-5">
                        {state.playlists.map((playlist) => (
                          <button
                            key={`${playlist.source_id}:${playlist.remote_id}`}
                            className="min-w-0 rounded-xl text-left focus-visible:outline-2"
                            onClick={() =>
                              void state.openPlaylist(playlist.remote_id)
                            }
                          >
                            <StreamingCover
                              url={playlist.cover_url}
                              size={200}
                              className="w-full rounded-xl"
                            />
                            <span className="mt-2 block truncate">
                              {playlist.name}
                            </span>
                            {playlist.track_count !== null ? (
                              <span className="text-xs text-[var(--color-text-dim)]">
                                {t("catalog.streaming.songCount", {
                                  count: playlist.track_count,
                                })}
                              </span>
                            ) : null}
                          </button>
                        ))}
                      </div>
                    ) : (
                      <>
                        {canImport &&
                        (tracks.length > 0 || (isDetail && detail)) ? (
                          <div className="mb-3 flex gap-2">
                            <button
                              className={button}
                              aria-pressed={state.selecting}
                              onClick={() =>
                                state.setSelecting(!state.selecting)
                              }
                            >
                              {t(
                                state.selecting
                                  ? "catalog.streaming.selectDone"
                                  : "catalog.streaming.select",
                              )}
                            </button>
                            {state.selecting ? (
                              <button
                                className={button}
                                disabled={
                                  state.selected.size === 0 ||
                                  state.importRun?.phase === "running"
                                }
                                onClick={() =>
                                  void state.importTracks(
                                    tracks
                                      .filter((track) =>
                                        state.selected.has(
                                          track.remote_track_id,
                                        ),
                                      )
                                      .map((track) => track.remote_track_id),
                                    isDetail ? detail?.remote_id : null,
                                    isDetail ? detail?.name : null,
                                  )
                                }
                              >
                                {t("catalog.streaming.importSelected", {
                                  selected: state.selected.size,
                                })}
                              </button>
                            ) : isDetail ? (
                              <button
                                className={button}
                                disabled={
                                  state.importRun?.phase === "running" ||
                                  status === "loading"
                                }
                                onClick={() => void state.importPlaylist()}
                              >
                                {t("catalog.streaming.importPlaylist")}
                              </button>
                            ) : null}
                          </div>
                        ) : null}
                        <StreamingTrackList
                          key={state.activeSourceId ?? ""}
                          tracks={tracks}
                          scrollRef={scroll}
                          selection={
                            canImport && state.selecting
                              ? {
                                  selected: state.selected,
                                  onChange: state.setSelection,
                                }
                              : undefined
                          }
                          loadingMore={
                            status === "loading" && tracks.length > 0
                          }
                        />
                      </>
                    )}
                    {status === "ready" &&
                    (active === "playlists" && !isDetail
                      ? state.playlists.length === 0
                      : tracks.length === 0) ? (
                      <p className="py-6">
                        {t(
                          `catalog.streaming.${active === "liked" ? "likedEmpty" : active === "playlists" ? "playlistsEmpty" : "resultsEmpty"}`,
                        )}
                      </p>
                    ) : null}
                    {cursor ? (
                      <button
                        className={`${button} mt-4`}
                        disabled={status === "loading"}
                        onClick={() => load(true)}
                      >
                        {t("catalog.streaming.loadMore")}
                      </button>
                    ) : null}
                  </>
                )}
              </div>
            ) : (
              <p>{t("catalog.streaming.noOperations")}</p>
            )}
          </>
        )}
      </div>
    </section>
  );
}
