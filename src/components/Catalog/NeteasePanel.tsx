import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { NeteaseSignIn } from "@/components/Catalog/NeteaseSignIn";
import { useCatalogStore } from "@/stores/catalog-store";
import type { StreamingTrack } from "@/types/ipc";

export function NeteasePanel() {
  const { t } = useTranslation();
  const session = useCatalogStore((s) => s.session);
  const liked = useCatalogStore((s) => s.liked);
  const playlists = useCatalogStore((s) => s.playlists);
  const playlistDetail = useCatalogStore((s) => s.playlistDetail);
  const searchResults = useCatalogStore((s) => s.searchResults);
  const importFailures = useCatalogStore((s) => s.importFailures);
  const loadSession = useCatalogStore((s) => s.loadSession);
  const signOut = useCatalogStore((s) => s.signOut);
  const loadLiked = useCatalogStore((s) => s.loadLiked);
  const loadPlaylists = useCatalogStore((s) => s.loadPlaylists);
  const openPlaylist = useCatalogStore((s) => s.openPlaylist);
  const search = useCatalogStore((s) => s.search);
  const importTracks = useCatalogStore((s) => s.importTracks);

  const [query, setQuery] = useState("");
  const showBrowse = !!session?.signed_in && !session.expired;

  useEffect(() => {
    void loadSession();
  }, [loadSession]);

  const tracks: StreamingTrack[] =
    playlistDetail?.tracks ??
    (searchResults.length > 0 ? searchResults : liked);

  return (
    <div className="flex h-full flex-col gap-3 overflow-auto px-2 pb-4">
      <div className="px-2 text-[13px] font-semibold text-[var(--color-text)]">
        {t("catalog.netease.title")}
      </div>
      {showBrowse ? (
        <div className="space-y-2 px-2">
          <p
            className="text-[12px] text-[var(--color-text)]"
            data-testid="netease-signed-in"
          >
            {t("catalog.netease.signedInAs", {
              name: session?.display_name ?? "",
            })}
          </p>
          <button
            type="button"
            onClick={() => void signOut()}
            className="rounded-md border border-[var(--color-border-light)] px-3 py-1.5 text-[12px]"
          >
            {t("catalog.netease.signOut")}
          </button>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              onClick={() => void loadLiked()}
              className="rounded-md border border-[var(--color-border-light)] px-3 py-1.5 text-[12px]"
            >
              {t("catalog.netease.liked")}
            </button>
            <button
              type="button"
              onClick={() => void loadPlaylists()}
              className="rounded-md border border-[var(--color-border-light)] px-3 py-1.5 text-[12px]"
            >
              {t("catalog.netease.playlists")}
            </button>
          </div>
          <form
            className="flex gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              void search(query);
            }}
          >
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t("catalog.netease.searchPlaceholder")}
              aria-label={t("catalog.netease.search")}
              className="min-w-0 flex-1 rounded-md border border-[var(--color-border-light)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px]"
            />
            <button
              type="submit"
              className="rounded-md border border-[var(--color-border-light)] px-3 py-1.5 text-[12px]"
            >
              {t("catalog.netease.search")}
            </button>
          </form>
          {playlists.length > 0 ? (
            <ul className="space-y-1">
              {playlists.map((playlist) => (
                <li key={playlist.remote_id}>
                  <button
                    type="button"
                    onClick={() => void openPlaylist(playlist.remote_id)}
                    className="w-full truncate rounded-md px-2 py-1 text-left text-[13px] hover:bg-[var(--sidebar-row-overlay-bg)]"
                  >
                    {playlist.name}
                  </button>
                </li>
              ))}
            </ul>
          ) : null}
          {playlistDetail ? (
            <button
              type="button"
              onClick={() =>
                void importTracks(
                  playlistDetail.tracks.map((track) => track.remote_track_id),
                  playlistDetail.remote_id,
                )
              }
              className="rounded-md border border-[var(--color-border-light)] px-3 py-1.5 text-[12px]"
            >
              {t("catalog.netease.importPlaylist")}
            </button>
          ) : null}
          <ul className="space-y-1">
            {tracks.map((track) => (
              <li
                key={track.remote_track_id}
                className="rounded-md border border-[var(--color-border)] px-2 py-1.5"
              >
                <div className="text-[13px] text-[var(--color-text)]">
                  {track.title}
                </div>
                <div className="text-[11px] text-[var(--color-text-dim)]">
                  {track.artist}
                </div>
                {track.refusal ? (
                  <div className="text-[11px] text-[var(--color-text-dim)]">
                    {t("catalog.netease.importRefusal")}
                  </div>
                ) : (
                  <button
                    type="button"
                    onClick={() => void importTracks([track.remote_track_id])}
                    className="mt-1 text-[12px] text-[var(--color-accent)]"
                  >
                    {t("catalog.netease.importTrack")}
                  </button>
                )}
              </li>
            ))}
          </ul>
          {importFailures.length > 0 ? (
            <ul className="space-y-1 text-[11px] text-[var(--color-text-dim)]">
              {importFailures.map((failure) => (
                <li key={`${failure.remote_track_id}-${failure.reason}`}>
                  {failure.title} — {failure.artist}
                </li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : (
        <NeteaseSignIn />
      )}
    </div>
  );
}
