import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type RefObject,
} from "react";
import { Search, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { NeteaseAccountMenu } from "@/components/Catalog/NeteaseAccountMenu";
import { NeteaseImportBar } from "@/components/Catalog/NeteaseImportBar";
import {
  NeteasePlaylistDetail,
  NeteasePlaylistGrid,
  NeteasePlaylistSkeleton,
} from "@/components/Catalog/NeteasePlaylists";
import { NeteaseSignIn } from "@/components/Catalog/NeteaseSignIn";
import { NeteaseSignInErrorBanner } from "@/components/Catalog/NeteaseSignInErrorBanner";
import { NeteaseTrackList } from "@/components/Catalog/NeteaseTrackList";
import { APP_SHORTCUTS, matchesShortcut } from "@/lib/app-shortcuts";
import {
  useCatalogStore,
  type LoadStatus,
  type NeteaseTab,
} from "@/stores/catalog-store";
import type { StreamingTrack } from "@/types/ipc";

const focusRing =
  "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-focus-ring)]";

function ListState({
  status,
  onRetry,
  children,
}: {
  status: LoadStatus;
  onRetry: () => void;
  children: React.ReactNode;
}) {
  const { t } = useTranslation();
  if (status === "idle" || status === "loading") {
    return (
      <div role="status" aria-label={t("common.loading")}>
        <NeteasePlaylistSkeleton />
      </div>
    );
  }
  if (status === "error") {
    return (
      <div role="alert" className="flex items-center gap-3 p-6 text-[13px]">
        <span className="text-[var(--color-text-dim)]">
          {t("catalog.netease.loadFailed")}
        </span>
        <button
          type="button"
          onClick={onRetry}
          className={`rounded-md border border-[var(--color-border-light)] px-3 py-1 ${focusRing}`}
        >
          {t("common.retry")}
        </button>
      </div>
    );
  }
  return <>{children}</>;
}

function TrackList({
  status,
  tracks,
  emptyLabel,
  onRetry,
  scrollRef,
  loadingMore,
}: {
  status: LoadStatus;
  tracks: StreamingTrack[];
  loadingMore?: boolean;
  scrollRef: RefObject<HTMLElement | null>;
  emptyLabel: string;
  onRetry: () => void;
}) {
  if (status === "ready" && tracks.length === 0) {
    return (
      <p className="p-6 text-[13px] text-[var(--color-text-dim)]">
        {emptyLabel}
      </p>
    );
  }
  return (
    <ListState status={status} onRetry={onRetry}>
      <NeteaseTrackList
        tracks={tracks}
        scrollRef={scrollRef}
        loadingMore={loadingMore}
      />
    </ListState>
  );
}

export function NeteaseBrowser() {
  const { t } = useTranslation();
  const session = useCatalogStore((s) => s.session);
  const tab = useCatalogStore((s) => s.browserTab);
  const setTab = useCatalogStore((s) => s.setBrowserTab);
  const liked = useCatalogStore((s) => s.liked);
  const likedStatus = useCatalogStore((s) => s.likedStatus);
  const likedLoadingMore = useCatalogStore((s) => s.likedLoadingMore);
  const playlists = useCatalogStore((s) => s.playlists);
  const playlistsStatus = useCatalogStore((s) => s.playlistsStatus);
  const searchResults = useCatalogStore((s) => s.searchResults);
  const searchStatus = useCatalogStore((s) => s.searchStatus);
  const searchQuery = useCatalogStore((s) => s.searchQuery);
  const selectedPlaylistId = useCatalogStore((s) => s.selectedPlaylistId);
  const loadLiked = useCatalogStore((s) => s.loadLiked);
  const loadPlaylists = useCatalogStore((s) => s.loadPlaylists);
  const search = useCatalogStore((s) => s.search);
  const clearSearch = useCatalogStore((s) => s.clearSearch);

  const signedIn = !!session?.signed_in && !session.expired;
  const [draft, setDraft] = useState(searchQuery);
  const inputRef = useRef<HTMLInputElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const tabRefs = useRef<Partial<Record<NeteaseTab, HTMLButtonElement | null>>>(
    {},
  );

  useEffect(() => {
    setDraft(searchQuery);
  }, [searchQuery]);

  useEffect(() => {
    if (!signedIn) return;
    if (tab === "liked" && likedStatus === "idle") void loadLiked();
    if (tab === "playlists" && playlistsStatus === "idle") {
      void loadPlaylists();
    }
  }, [signedIn, tab, likedStatus, playlistsStatus, loadLiked, loadPlaylists]);

  useEffect(() => {
    if (!signedIn) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (matchesShortcut(APP_SHORTCUTS.findInView, event)) {
        event.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [signedIn]);

  const tabs: NeteaseTab[] = searchQuery
    ? ["liked", "playlists", "results"]
    : ["liked", "playlists"];
  const activeTab = tabs.includes(tab) ? tab : "playlists";

  const onTabKeyDown = (event: ReactKeyboardEvent, current: NeteaseTab) => {
    const index = tabs.indexOf(current);
    let next: number | null = null;
    if (event.key === "ArrowRight") next = (index + 1) % tabs.length;
    else if (event.key === "ArrowLeft") {
      next = (index - 1 + tabs.length) % tabs.length;
    } else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = tabs.length - 1;
    if (next === null) return;
    event.preventDefault();
    setTab(tabs[next]);
    tabRefs.current[tabs[next]]?.focus();
  };

  const tabLabel = (id: NeteaseTab) => {
    if (id === "liked") return t("catalog.netease.liked");
    if (id === "playlists") return t("catalog.netease.playlists");
    return t("catalog.netease.resultsTab", { query: searchQuery });
  };

  // Each view starts at the top; the playlist page replaces the grid in place.
  useEffect(() => {
    scrollRef.current?.scrollTo?.({ top: 0 });
  }, [activeTab, selectedPlaylistId]);

  const submitSearch = () => void search(draft);

  return (
    <div
      data-testid="netease-browser"
      className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-[var(--color-surface)]"
    >
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto pb-20">
        <NeteaseSignInErrorBanner />
        {signedIn ? (
          <>
            <div className="flex flex-wrap items-center gap-x-6 gap-y-2 px-6 pb-2 pt-5">
              <div
                role="tablist"
                aria-label={t("catalog.netease.tabsLabel")}
                className="flex min-w-0 flex-1 basis-60 items-center gap-5"
              >
                {tabs.map((id) => (
                  <button
                    key={id}
                    ref={(node) => {
                      tabRefs.current[id] = node;
                    }}
                    type="button"
                    role="tab"
                    id={`netease-tab-${id}`}
                    aria-selected={activeTab === id}
                    aria-controls="netease-tabpanel"
                    tabIndex={activeTab === id ? 0 : -1}
                    onClick={() => setTab(id)}
                    onKeyDown={(event) => onTabKeyDown(event, id)}
                    className={`max-w-[260px] truncate rounded text-[18px] font-bold ${focusRing} ${
                      activeTab === id
                        ? "text-[var(--color-text)]"
                        : "text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
                    }`}
                  >
                    {tabLabel(id)}
                  </button>
                ))}
              </div>
              <form
                role="search"
                className="relative w-56"
                onSubmit={(event) => {
                  event.preventDefault();
                  submitSearch();
                }}
              >
                <Search
                  size={14}
                  aria-hidden="true"
                  className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-[var(--color-text-dim)]"
                />
                <input
                  ref={inputRef}
                  value={draft}
                  onChange={(event) => setDraft(event.target.value)}
                  placeholder={t("catalog.netease.searchPlaceholder")}
                  aria-label={t("catalog.netease.searchPlaceholder")}
                  className={`min-h-9 w-full rounded-full bg-[var(--color-surface-muted)] pl-8 pr-8 text-[13px] text-[var(--color-text)] placeholder:text-[var(--color-text-dim)] ${focusRing}`}
                />
                {draft ? (
                  <button
                    type="button"
                    aria-label={t("catalog.netease.clearSearch")}
                    onClick={() => {
                      setDraft("");
                      if (searchQuery) clearSearch();
                      inputRef.current?.focus();
                    }}
                    className={`absolute right-1.5 top-1/2 inline-flex size-6 -translate-y-1/2 items-center justify-center rounded-full text-[var(--color-text-dim)] hover:bg-[var(--color-ghost-hover)] ${focusRing}`}
                  >
                    <X size={12} aria-hidden="true" />
                  </button>
                ) : null}
              </form>
              <NeteaseAccountMenu name={session?.display_name ?? ""} />
            </div>

            <div
              role="tabpanel"
              id="netease-tabpanel"
              aria-labelledby={`netease-tab-${activeTab}`}
            >
              {activeTab === "liked" ? (
                <TrackList
                  scrollRef={scrollRef}
                  status={likedStatus}
                  tracks={liked}
                  loadingMore={likedLoadingMore}
                  emptyLabel={t("catalog.netease.likedEmpty")}
                  onRetry={() => void loadLiked()}
                />
              ) : null}
              {activeTab === "playlists" ? (
                selectedPlaylistId ? (
                  <NeteasePlaylistDetail
                    scrollRef={scrollRef}
                    fallbackName={
                      playlists.find((p) => p.remote_id === selectedPlaylistId)
                        ?.name ?? ""
                    }
                  />
                ) : playlistsStatus === "ready" && playlists.length === 0 ? (
                  <p className="p-6 text-[13px] text-[var(--color-text-dim)]">
                    {t("catalog.netease.playlistsEmpty")}
                  </p>
                ) : (
                  <ListState
                    status={playlistsStatus}
                    onRetry={() => void loadPlaylists()}
                  >
                    <NeteasePlaylistGrid playlists={playlists} />
                  </ListState>
                )
              ) : null}
              {activeTab === "results" ? (
                <TrackList
                  scrollRef={scrollRef}
                  status={searchStatus}
                  tracks={searchResults}
                  emptyLabel={t("catalog.netease.resultsEmpty", {
                    query: searchQuery,
                  })}
                  onRetry={submitSearch}
                />
              ) : null}
            </div>
          </>
        ) : (
          <NeteaseSignIn />
        )}
      </div>
      <NeteaseImportBar />
    </div>
  );
}
