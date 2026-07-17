import i18next from "@/lib/i18n";
import { useLayoutStore } from "@/stores/layout-store";
import { useLibraryStore } from "@/stores/library-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";
import { usePlaylistStore } from "@/stores/playlist-store";
import { useQueueStore } from "@/stores/queue-store";
import { useSettingsStore } from "@/stores/settings-store";
import type { PlaylistSong } from "@/lib/tauri/playlist";
import type { LyricLine, PlaybackStateSnapshot, Song } from "@/types/ipc";

const EMPTY_COVER = Uint8Array.from(
  atob(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
  ),
  (character) => character.charCodeAt(0),
);

function song(
  hash: string,
  title: string,
  artist: string,
  durationMs: number,
  language: string,
): Song {
  return {
    hash,
    file_path: null,
    audio_source_kind: "original",
    cdg_path: null,
    media_g_container: null,
    instrumental: false,
    language,
    title,
    artist,
    album: null,
    duration_ms: durationMs,
    cover_art: EMPTY_COVER,
    has_cover_art: true,
    imported_at: 1,
    original_ext: "flac",
  };
}

export const PREVIEW_SONGS = [
  song("hachiko", "Hachikō", "Fujii Kaze", 270_000, "ja"),
  song("aria", "Aria（カラオケ）", "平沢進", 284_000, "ja"),
  song("fly-day", "Fly-day Chinatown", "泰葉", 210_000, "ja"),
  song("kirari", "Kirari", "Fujii Kaze", 231_000, "ja"),
  song("love-like-this", "Love Like This", "Fujii Kaze", 260_000, "en"),
  song("matsuri", "Matsuri", "Fujii Kaze", 225_000, "ja"),
  song("cause", "因果", "擦除SKAI ISYOURGOD", 151_000, "zh"),
  song("lover", "爱人错过", "告五人", 292_000, "zh"),
];

const PREVIEW_PLAYLISTS = [
  {
    id: "favorites",
    name: "Favorites",
    song_count: 12,
    created_at: 1,
    updated_at: 1,
  },
  {
    id: "party",
    name: "Friday night",
    song_count: 28,
    created_at: 1,
    updated_at: 1,
  },
] as const;

const PREVIEW_PLAYLIST_SONGS: Record<string, string[]> = {
  favorites: ["hachiko", "aria", "love-like-this", "lover"],
  party: ["fly-day", "kirari", "matsuri", "cause"],
};

const LYRICS: LyricLine[] = [
  {
    time_ms: 24_000,
    text: "While everybody's screamin' shoutin'",
    words: null,
    bg_words: null,
    section: null,
  },
  {
    time_ms: 30_000,
    text: "We're so chill out here just vibin'",
    words: null,
    bg_words: null,
    section: null,
  },
  {
    time_ms: 36_000,
    text: "Tryin' to spread this peacefulness with y'all",
    words: null,
    bg_words: null,
    section: null,
  },
  {
    time_ms: 46_000,
    text: "Our holiday's just getting started",
    words: null,
    bg_words: null,
    section: null,
  },
  {
    time_ms: 53_000,
    text: "Just be kind and open hearted",
    words: null,
    bg_words: null,
    section: null,
  },
  {
    time_ms: 61_000,
    text: "Feel the breeze and let God bless us all",
    words: null,
    bg_words: null,
    section: null,
  },
  {
    time_ms: 70_000,
    text: "Doko ni ikō Hachikō",
    words: null,
    bg_words: null,
    section: null,
  },
  {
    time_ms: 77_000,
    text: "Doko ni ikō Hachikō",
    words: null,
    bg_words: null,
    section: null,
  },
  {
    time_ms: 84_000,
    text: "You've been patiently waiting for me",
    words: null,
    bg_words: null,
    section: null,
  },
];

function snapshot(
  songId = "hachiko",
  overrides: Partial<PlaybackStateSnapshot> = {},
): PlaybackStateSnapshot {
  const current = PREVIEW_SONGS.find((item) => item.hash === songId);
  return {
    song_id: songId,
    transport_generation: 1,
    state: "playing",
    is_playing: false,
    position_ms: 61_000,
    duration_ms: current?.duration_ms ?? 270_000,
    buffered_ms: current?.duration_ms ?? 270_000,
    volume: 0.82,
    stem_volumes: { vocals: 0.44, drums: 0.78, bass: 0.72, other: 0.76 },
    has_stems: true,
    stem_mode: "four_stem",
    ...overrides,
  };
}

let initialized = false;

export function initializeMockApp(language: "en" | "zh-CN") {
  void i18next.changeLanguage(language);

  if (initialized) {
    return;
  }
  initialized = true;

  useLayoutStore.setState({ sidebarVisible: true, sidebarWidth: 260 });
  useSettingsStore.setState({
    isOpen: false,
    hydrated: true,
    stemMode: "four_stem",
    hideBatchSeparate: false,
    coverArtBackdrop: false,
  });
  useQueueStore.setState({ queue: [], playHistory: [], isOpen: false });
  usePlaylistStore.setState({
    playlists: [...PREVIEW_PLAYLISTS],
    activePlaylistId: "favorites",
    isLoading: false,
    playlistSongSets: new Map(
      Object.entries(PREVIEW_PLAYLIST_SONGS).map(([playlistId, songIds]) => [
        playlistId,
        new Set(songIds),
      ]),
    ),
    loadPlaylists: async () => {},
    loadPlaylistSongSets: async () => {},
    createPlaylist: async () => PREVIEW_PLAYLISTS[0],
    renamePlaylist: async () => {},
    deletePlaylist: async () => {},
    addSongsToPlaylist: async () => {},
    removeSongsFromPlaylist: async () => {},
    getPlaylistSongs: async (playlistId): Promise<PlaylistSong[]> =>
      (PREVIEW_PLAYLIST_SONGS[playlistId] ?? []).map((song_hash, index) => ({
        song_hash,
        added_at: index + 1,
        sort_order: index,
        singer: null,
      })),
  });
  useLibraryStore.setState({
    songs: PREVIEW_SONGS,
    searchQuery: "",
    filter: "all",
    selectedSongIds: new Set(["hachiko"]),
    lastClickedSongId: "hachiko",
    separationStatuses: {},
    uploadStatuses: {},
    setSearchQuery: () => {},
    searchSongs: async () => {},
    importFiles: async () => {},
  });
  useLyricsStore.setState({
    songId: "hachiko",
    lines: LYRICS,
    source: "embedded",
    offsetMs: 0,
    rawLrc: LYRICS.map((line) => line.text).join("\n"),
    activeLineIndex: 5,
    activeWordIndex: -1,
    isLoading: false,
    setOffset: async (_songId, ms) => useLyricsStore.setState({ offsetMs: ms }),
    adjustOffset: async (_songId, deltaMs) =>
      useLyricsStore.setState((state) => ({
        offsetMs: state.offsetMs + deltaMs,
      })),
  });
  usePlayerStore.setState({
    snapshot: snapshot(),
    positionMs: 61_000,
    playingSinceMs: null,
    playSong: async () => {},
    playNow: async () => {},
    resume: async () => {},
    pause: async () => {},
    seek: async () => {},
    setVolume: async () => {},
    setStemVolume: async () => {},
    skipForward: async () => {},
    skipBack: async () => {},
  });
}
