import { useEffect, useRef, useState, type ReactNode } from "react";
import { ChevronLeft, Download, Loader2, Search, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { APP_SHORTCUTS, matchesShortcut } from "@/lib/app-shortcuts";
import {
  canMatchInPlaylist,
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
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

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
    source.loaded &&
    ![
      "not_installed",
      "disabled",
      "update_required",
      "unavailable",
      "cancelling",
    ].includes(source.state);
  const challengeId =
    state.sourceStates[state.activeSourceId ?? ""]?.challenge?.challenge;
  const signInCard = useRef<HTMLDivElement>(null);
  const headerSignIn = useRef<HTMLButtonElement>(null);
  const [starting, setStarting] = useState(false);
  const hadChallenge = useRef(false);
  useEffect(() => {
    if (challengeId) signInCard.current?.focus();
    else if (hadChallenge.current) headerSignIn.current?.focus();
    hadChallenge.current = !!challengeId;
  }, [challengeId]);
  const draft = state.searchDraft;
  const setDraft = state.setSearchDraft;
  const input = useRef<HTMLInputElement>(null);
  const scroll = useRef<HTMLDivElement>(null);
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
  const matchableCount = isDetail
    ? tracks.filter((track) =>
        canMatchInPlaylist(state.sources, track, state.trackStates),
      ).length
    : 0;
  const canImport = sourceAllows(source, "import", state.session);
  const authDeclared = source?.manifest?.authentication;
  const hasAuth =
    !!authDeclared &&
    typeof authDeclared === "object" &&
    "flow" in authDeclared &&
    authDeclared.flow !== "none";
  const signInCardElement = (
    <div ref={signInCard} tabIndex={-1} className="outline-none">
      <StreamingSignIn key={state.activeSourceId ?? ""} />
    </div>
  );
  const importBusy = state.importRun?.phase === "running";
  const emptyKey = isDetail
    ? "playlistTracksEmpty"
    : active === "liked"
      ? "likedEmpty"
      : active === "playlists"
        ? "playlistsEmpty"
        : "resultsEmpty";
  const errorKey = isDetail
    ? "playlist"
    : active === "liked"
      ? "liked"
      : active === "playlists"
        ? "playlists"
        : "search";
  const empty =
    status === "ready" &&
    (active === "playlists" && !isDetail
      ? state.playlists.length === 0
      : tracks.length === 0);
  const detailMeta = [
    detail?.creator
      ? t("catalog.streaming.byCreator", { creator: detail.creator })
      : null,
    detail?.track_count !== null && detail?.track_count !== undefined
      ? t("catalog.streaming.songCount", { count: detail.track_count })
      : null,
    detail?.updated_at_ms
      ? t("catalog.streaming.updatedOn", {
          date: formatUpdated(detail.updated_at_ms, i18n.language),
        })
      : null,
  ].filter(Boolean);
  return (
    <section
      data-testid="streaming-browser"
      aria-label={source?.name ?? t("catalog.streaming.unavailable")}
      className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-background"
    >
      <div
        ref={scroll}
        className="min-h-0 flex-1 overflow-auto px-6 pb-24 pt-5"
      >
        <header className="mb-5 flex min-h-9 items-center gap-3">
          <h1 className="min-w-0 truncate text-2xl font-semibold tracking-tight">
            {source?.name ??
              state.activeSourceId ??
              t("catalog.streaming.unavailable")}
          </h1>
          <div className="ml-auto shrink-0">
            {state.session?.signed_in && !state.session.expired ? (
              <StreamingAccountMenu
                key={state.activeSourceId ?? ""}
                name={state.session.display_name ?? ""}
              />
            ) : hasAuth && ready && available && !challengeId ? (
              <Button
                ref={headerSignIn}
                disabled={starting || state.authStatus === "loading"}
                onClick={async () => {
                  setStarting(true);
                  try {
                    await state.openSignIn();
                  } finally {
                    setStarting(false);
                  }
                }}
              >
                {t("catalog.streaming.connect")}
              </Button>
            ) : null}
          </div>
        </header>
        <StreamingSignInErrorBanner />
        {state.sessionStatus === "error" ? (
          <Notice>
            <p>{state.errors.session}</p>
            <Button variant="outline" onClick={() => void state.loadSession()}>
              {t("common.retry")}
            </Button>
          </Notice>
        ) : null}
        {!ready ? (
          <p role="status" className="text-muted-foreground">
            {t("catalog.streaming.unavailable")}
          </p>
        ) : (
          <>
            {available && challengeId ? signInCardElement : null}
            {capabilities.includes("search") ? (
              <form
                role="search"
                className="mb-4 flex gap-2"
                onSubmit={(event) => {
                  event.preventDefault();
                  void state.search(draft);
                }}
              >
                <div className="relative min-w-0 flex-1">
                  <Search
                    aria-hidden="true"
                    className="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
                  />
                  <Input
                    ref={input}
                    aria-label={t("catalog.streaming.searchPlaceholder")}
                    placeholder={t("catalog.streaming.searchPlaceholder")}
                    value={draft}
                    onChange={(event) => setDraft(event.target.value)}
                    className="h-9 pl-8 pr-9"
                  />
                  {draft ? (
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      aria-label={t("catalog.streaming.clearSearch")}
                      className="absolute right-0.5 top-1/2 -translate-y-1/2"
                      onClick={() => {
                        setDraft("");
                        state.clearSearch();
                        input.current?.focus();
                      }}
                    >
                      <X />
                    </Button>
                  ) : null}
                </div>
                <Button
                  type="submit"
                  variant="secondary"
                  size="lg"
                  disabled={!sourceAllows(source, "search", state.session)}
                >
                  {t("catalog.streaming.search")}
                </Button>
              </form>
            ) : null}
            {active ? (
              <Tabs
                value={active}
                onValueChange={(value) =>
                  state.setBrowserTab(value as StreamingTab)
                }
                className="gap-4"
              >
                <TabsList
                  variant="line"
                  activateOnFocus
                  aria-label={t("catalog.streaming.tabsLabel")}
                >
                  {tabs.map((tab) => (
                    <TabsTrigger key={tab} value={tab} className="px-3">
                      {t(
                        `catalog.streaming.${tab === "results" ? "results" : tab}`,
                      )}
                    </TabsTrigger>
                  ))}
                </TabsList>
                <TabsContent value={active}>
                  {isDetail ? (
                    <Button
                      variant="ghost"
                      className="-ml-2.5 mb-3 text-muted-foreground"
                      onClick={state.closePlaylist}
                    >
                      <ChevronLeft />
                      {t("catalog.streaming.back")}
                    </Button>
                  ) : null}
                  {!available ? (
                    signInCardElement
                  ) : (
                    <>
                      {isDetail ? (
                        <div className="mb-5">
                          <div className="flex items-end gap-5">
                            <StreamingCover
                              url={detail?.cover_url ?? null}
                              size={160}
                              glow
                              eager
                              className="size-32 rounded-xl"
                            />
                            <div className="min-w-0 flex-1">
                              <h2 className="truncate text-2xl font-semibold tracking-tight">
                                {detail?.name}
                              </h2>
                              {detailMeta.length > 0 ? (
                                <p className="mt-1 text-muted-foreground">
                                  {detailMeta.join(" · ")}
                                </p>
                              ) : null}
                              {detail?.description ? (
                                <p className="mt-1 line-clamp-2 text-muted-foreground">
                                  {detail.description}
                                </p>
                              ) : null}
                            </div>
                          </div>
                        </div>
                      ) : null}
                      {status === "error" ? (
                        <Notice>
                          <p className="font-medium">
                            {t("catalog.streaming.loadFailed")}
                          </p>
                          <p className="text-muted-foreground">
                            {state.errors[errorKey]}
                          </p>
                          <Button variant="outline" onClick={() => load()}>
                            {t("common.retry")}
                          </Button>
                        </Notice>
                      ) : null}
                      {active === "playlists" && !isDetail ? (
                        <div className="-mx-2 grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-2">
                          {state.playlists.map((playlist) => (
                            <button
                              key={`${playlist.source_id}:${playlist.remote_id}`}
                              className="min-w-0 rounded-xl p-2 text-left outline-none transition-colors hover:bg-accent focus-visible:ring-3 focus-visible:ring-ring/50"
                              onClick={() =>
                                void state.openPlaylist(playlist.remote_id)
                              }
                            >
                              <StreamingCover
                                url={playlist.cover_url}
                                size={200}
                                className="w-full rounded-lg"
                              />
                              <span className="mt-2 block truncate font-medium">
                                {playlist.name}
                              </span>
                              {playlist.track_count !== null ? (
                                <span className="block text-xs text-muted-foreground">
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
                            <div className="mb-3 flex flex-wrap gap-2">
                              {state.selecting ? (
                                <Button
                                  disabled={
                                    state.selected.size === 0 || importBusy
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
                                </Button>
                              ) : isDetail ? (
                                <>
                                  <Button
                                    disabled={
                                      importBusy || status === "loading"
                                    }
                                    onClick={() => void state.importPlaylist()}
                                  >
                                    <Download />
                                    {t("catalog.streaming.importPlaylist")}
                                  </Button>
                                  {matchableCount > 0 ? (
                                    <Button
                                      variant="outline"
                                      disabled={
                                        importBusy || status === "loading"
                                      }
                                      onClick={() =>
                                        void state.findPlaylistMatches()
                                      }
                                    >
                                      {t(
                                        "catalog.streaming.playlistMatch.find",
                                        { count: matchableCount },
                                      )}
                                    </Button>
                                  ) : null}
                                </>
                              ) : null}
                              <Button
                                variant="outline"
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
                              </Button>
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
                      {status === "loading" ? (
                        <p
                          role="status"
                          className="flex items-center gap-2 py-6 text-muted-foreground"
                        >
                          <Loader2
                            aria-hidden="true"
                            className="size-4 animate-spin"
                          />
                          {t("common.loading")}
                        </p>
                      ) : null}
                      {empty ? (
                        <p className="py-10 text-center text-muted-foreground">
                          {t(`catalog.streaming.${emptyKey}`, {
                            query: state.searchQuery,
                          })}
                        </p>
                      ) : null}
                      {cursor ? (
                        <div className="mt-4 flex justify-center">
                          <Button
                            variant="outline"
                            disabled={status === "loading"}
                            onClick={() => load(true)}
                          >
                            {t("catalog.streaming.loadMore")}
                          </Button>
                        </div>
                      ) : null}
                    </>
                  )}
                </TabsContent>
              </Tabs>
            ) : (
              <p className="text-muted-foreground">
                {t("catalog.streaming.noOperations")}
              </p>
            )}
          </>
        )}
      </div>
    </section>
  );
}

function Notice({ children }: { children: ReactNode }) {
  return (
    <div
      role="alert"
      className="mb-4 flex flex-col items-start gap-2 rounded-lg border border-border bg-card p-4"
    >
      {children}
    </div>
  );
}
