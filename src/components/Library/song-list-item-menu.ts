import type { ContextMenuItem } from "./ContextMenu";

type TranslateFn = (
  key: string,
  options?: Record<string, string | number>,
) => string;

export type SongLanguage =
  | "mandarin"
  | "cantonese"
  | "japanese"
  | "korean"
  | "cyrillic"
  | "thai"
  | "devanagari"
  | "gujarati"
  | "gurmukhi"
  | "telugu"
  | "kannada"
  | "odia"
  | "tamil";

export const SONG_LANGUAGES: SongLanguage[] = [
  "mandarin",
  "cantonese",
  "japanese",
  "korean",
  "cyrillic",
  "thai",
  "devanagari",
  "gujarati",
  "gurmukhi",
  "telugu",
  "kannada",
  "odia",
  "tamil",
];

interface BuildSongListContextMenuItemsArgs {
  t: TranslateFn;
  isMultiSelected: boolean;
  selectedCount: number;
  selectedSongIds: string[];
  selectedHasSeparableSongs: boolean;
  selectedCanToggleInstrumentalSongs: boolean;
  selectedInstrumentalState: "checked" | "mixed" | "unchecked";
  selectedLanguage: SongLanguage | null;
  setSelectedLanguage: (language: SongLanguage | null) => void;
  supportsEmbeddedLyrics: boolean;
  queueAllSelected: () => void;
  separateAllSelected: () => void;
  toggleSelectedInstrumental: () => void;
  extractSelectedEmbeddedCoverArt: () => void;
  deleteSelected: () => void;
  playNow: () => void;
  playNext: () => void;
  addToQueue: () => void;
  extractEmbeddedCoverArt: () => void;
  extractEmbeddedLyrics: () => void;
  fetchLyricsOnline: () => void;
  editInfo: () => void;
  openProperties: () => void;
  deleteSong: () => void;
  playlists: Array<{ id: string; name: string }>;
  songPlaylistMembership: Map<string, "checked" | "mixed" | null>;
  onAddToPlaylist: (playlistId: string) => void;
  onRemoveFromPlaylist: (playlistId: string) => void;
  onCreatePlaylistAndAdd: () => void;
  activePlaylistId: string | null;
  onRemoveFromActivePlaylist: () => void;
}

export function buildSongListContextMenuItems({
  t,
  isMultiSelected,
  selectedCount,
  selectedSongIds,
  selectedHasSeparableSongs,
  selectedCanToggleInstrumentalSongs,
  selectedInstrumentalState,
  selectedLanguage,
  setSelectedLanguage,
  supportsEmbeddedLyrics,
  queueAllSelected,
  separateAllSelected,
  toggleSelectedInstrumental,
  extractSelectedEmbeddedCoverArt,
  deleteSelected,
  playNow,
  playNext,
  addToQueue,
  extractEmbeddedCoverArt,
  extractEmbeddedLyrics,
  fetchLyricsOnline,
  editInfo,
  openProperties,
  deleteSong,
  playlists,
  songPlaylistMembership,
  onAddToPlaylist,
  onRemoveFromPlaylist,
  onCreatePlaylistAndAdd,
  activePlaylistId,
  onRemoveFromActivePlaylist,
}: BuildSongListContextMenuItemsArgs): ContextMenuItem[] {
  const addToPlaylistChildren: ContextMenuItem[] = [
    ...playlists.map((p) => {
      const membership = songPlaylistMembership.get(p.id) ?? null;
      return {
        label: p.name,
        indicator: membership,
        onClick: () => {
          if (membership === "checked") {
            onRemoveFromPlaylist(p.id);
          } else {
            onAddToPlaylist(p.id);
          }
        },
      };
    }),
    {
      label: t("playlist.newPlaylist"),
      onClick: onCreatePlaylistAndAdd,
    },
  ];

  const addToPlaylistItem: ContextMenuItem = {
    label: t("playlist.addTo"),
    children: addToPlaylistChildren,
  };

  const removeFromPlaylistItem: ContextMenuItem | null = activePlaylistId
    ? {
        label: t("playlist.removeFromPlaylist"),
        onClick: onRemoveFromActivePlaylist,
      }
    : null;

  if (isMultiSelected) {
    const instrumentalIndicator: ContextMenuItem["indicator"] =
      selectedInstrumentalState === "checked"
        ? "checked"
        : selectedInstrumentalState === "mixed"
          ? "mixed"
          : null;

    const languageChildren: ContextMenuItem[] = [
      {
        label: t("library.languageAuto"),
        onClick: () => setSelectedLanguage(null),
        indicator: selectedLanguage === null ? "checked" : null,
      },
      ...SONG_LANGUAGES.map((lang) => ({
        label: t(`library.language_${lang}`),
        onClick: () => setSelectedLanguage(lang),
        indicator: (selectedLanguage === lang
          ? "checked"
          : null) as ContextMenuItem["indicator"],
      })),
    ];

    return [
      {
        label: t("library.queueAllSelected", {
          count: selectedCount || selectedSongIds.length,
        }),
        onClick: queueAllSelected,
      },
      addToPlaylistItem,
      ...(selectedCanToggleInstrumentalSongs
        ? [
            {
              label: t("library.markInstrumentalSelected", {
                count: selectedCount || selectedSongIds.length,
              }),
              onClick: toggleSelectedInstrumental,
              indicator: instrumentalIndicator,
            },
          ]
        : []),
      ...(selectedHasSeparableSongs
        ? [
            {
              label: t("library.separateAllSelected", {
                count: selectedCount || selectedSongIds.length,
              }),
              onClick: separateAllSelected,
            },
          ]
        : []),
      {
        label: t("library.language"),
        children: languageChildren,
      },
      {
        label: t("library.extractEmbeddedCoverArtSelected", {
          count: selectedCount || selectedSongIds.length,
        }),
        onClick: extractSelectedEmbeddedCoverArt,
      },
      ...(removeFromPlaylistItem ? [removeFromPlaylistItem] : []),
      {
        label: t("library.deleteSelected", {
          count: selectedCount || selectedSongIds.length,
        }),
        onClick: deleteSelected,
      },
    ];
  }

  const languageChildren: ContextMenuItem[] = [
    {
      label: t("library.languageAuto"),
      onClick: () => setSelectedLanguage(null),
      indicator: selectedLanguage === null ? "checked" : null,
    },
    ...SONG_LANGUAGES.map((lang) => ({
      label: t(`library.language_${lang}`),
      onClick: () => setSelectedLanguage(lang),
      indicator: (selectedLanguage === lang
        ? "checked"
        : null) as ContextMenuItem["indicator"],
    })),
  ];

  const baseItems: ContextMenuItem[] = [
    {
      label: t("library.playNow"),
      onClick: playNow,
    },
    {
      label: t("library.playNext"),
      onClick: playNext,
    },
    {
      label: t("library.addToQueue"),
      onClick: addToQueue,
    },
    addToPlaylistItem,
    {
      label: t("library.extractEmbeddedCoverArt"),
      onClick: extractEmbeddedCoverArt,
    },
    ...(supportsEmbeddedLyrics
      ? [
          {
            label: t("library.extractEmbeddedLyrics"),
            onClick: extractEmbeddedLyrics,
          },
        ]
      : []),
    {
      label: t("library.fetchLyricsOnline"),
      onClick: fetchLyricsOnline,
    },
    {
      label: t("library.language"),
      children: languageChildren,
    },
    {
      label: t("library.editInfo"),
      onClick: editInfo,
    },
    {
      label: t("library.properties"),
      onClick: openProperties,
    },
    ...(removeFromPlaylistItem ? [removeFromPlaylistItem] : []),
    {
      label: t("library.delete"),
      onClick: deleteSong,
    },
  ];

  return baseItems;
}
