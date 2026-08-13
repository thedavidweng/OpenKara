import { beforeEach, describe, expect, test, vi } from "vitest";
import type { ContextMenuItem } from "@/components/Library/ContextMenu";
import { createMockBackend } from "@/lib/backend/mock-backend";
import {
  createRecordingSongCommandPorts,
  type RecordingSongCommandPorts,
} from "@/test-utils/song-commands";
import type { Song } from "@/types/ipc";
import { createSongCommands } from "./song-commands";
import type { SongCommandContext, SongCommands } from "./types";

const { mockNotifyError, mockNotifySuccess } = vi.hoisted(() => ({
  mockNotifyError: vi.fn(),
  mockNotifySuccess: vi.fn(),
}));

vi.mock("@/lib/errors", () => ({
  notifyError: mockNotifyError,
  notifySuccess: mockNotifySuccess,
}));

function makeSong(overrides: Partial<Song> = {}): Song {
  return {
    hash: "song-abc",
    title: "Test Song",
    artist: "Test Artist",
    album: null,
    file_path: "/music/test.mp3",
    audio_source_kind: "original",
    cdg_path: null,
    media_g_container: null,
    instrumental: false,
    language: null,
    duration_ms: 180_000,
    cover_art: null,
    has_cover_art: false,
    artwork_thumb_path: null,
    imported_at: 0,
    original_ext: null,
    ...overrides,
  };
}

const t = (key: string, options?: Record<string, string | number>) =>
  options?.count === undefined ? key : `${key}:${options.count}`;

const mockBatchSeparate = vi.fn().mockResolvedValue(undefined);
const mockExtractEmbeddedLyrics = vi.fn().mockResolvedValue(undefined);
const mockFetchLyricsOnline = vi.fn().mockResolvedValue({ lines: [] });

const backend = createMockBackend({
  overrides: {
    maintenance: { batchSeparate: mockBatchSeparate },
    lyrics: {
      extractEmbeddedLyrics: mockExtractEmbeddedLyrics,
      fetchLyricsOnline: mockFetchLyricsOnline,
    },
  },
});

let ports: RecordingSongCommandPorts;
let commands: SongCommands;

function context(song: Song = makeSong()): SongCommandContext {
  return { song, t };
}

function setUp(seed: Parameters<typeof createRecordingSongCommandPorts>[0]) {
  ports = createRecordingSongCommandPorts(seed);
  commands = createSongCommands({
    backend,
    stores: ports.stores,
    dialogs: ports.dialogs,
  });
}

function labels(items: ContextMenuItem[]): string[] {
  return items.map((item) => item.label);
}

function item(items: ContextMenuItem[], label: string): ContextMenuItem {
  const found = items.find((entry) => entry.label === label);
  if (!found) throw new Error(`no menu item labelled ${label}`);
  return found;
}

function submenu(items: ContextMenuItem[], label: string): ContextMenuItem[] {
  return item(items, label).children ?? [];
}

const THREE_SELECTED = {
  selectedSongIds: ["song-abc", "song-def", "song-ghi"],
  songs: [
    makeSong(),
    makeSong({ hash: "song-def" }),
    makeSong({ hash: "song-ghi" }),
  ],
};

beforeEach(() => {
  vi.clearAllMocks();
  mockBatchSeparate.mockResolvedValue(undefined);
  mockExtractEmbeddedLyrics.mockResolvedValue(undefined);
  mockFetchLyricsOnline.mockResolvedValue({ lines: [] });
  setUp({});
});

describe("buildMenu – which menu the selection earns", () => {
  test("offers the single-song menu when the row is the only context song", () => {
    const menu = labels(commands.buildMenu(context()));

    expect(menu).toContain("library.playNow");
    expect(menu).toContain("library.delete");
    expect(menu).not.toContain("library.deleteSelected:1");
  });

  test("offers the multi-select menu when the row is part of a wider selection", () => {
    setUp(THREE_SELECTED);

    const menu = labels(commands.buildMenu(context()));

    expect(menu).toContain("library.queueAllSelected:3");
    expect(menu).toContain("library.deleteSelected:3");
    expect(menu).not.toContain("library.playNow");
  });

  test("treats a selection the row is absent from as a single-song menu", () => {
    setUp({
      selectedSongIds: ["song-def", "song-ghi"],
      songs: [makeSong({ hash: "song-def" }), makeSong({ hash: "song-ghi" })],
    });

    const menu = labels(commands.buildMenu(context()));

    expect(menu).toContain("library.playNow");
    expect(menu).not.toContain("library.queueAllSelected:2");
  });

  test("hides embedded-lyrics extraction for zip-packaged media", () => {
    const menu = labels(
      commands.buildMenu(context(makeSong({ media_g_container: "zip" }))),
    );

    expect(menu).not.toContain("library.extractEmbeddedLyrics");
  });

  test("offers embedded-lyrics extraction for songs outside a zip container", () => {
    const menu = labels(
      commands.buildMenu(context(makeSong({ media_g_container: null }))),
    );

    expect(menu).toContain("library.extractEmbeddedLyrics");
  });
});

describe("buildMenu – indicators", () => {
  test("omits the instrumental toggle when no context song supports the flag", () => {
    setUp({
      selectedSongIds: ["song-abc", "song-def"],
      songs: [
        makeSong({ cdg_path: "/music/abc.cdg" }),
        makeSong({ hash: "song-def", cdg_path: "/music/def.cdg" }),
      ],
    });

    expect(labels(commands.buildMenu(context()))).not.toContain(
      "library.markInstrumentalSelected:2",
    );
  });

  test("checks the instrumental toggle when every capable song is instrumental", () => {
    setUp({
      selectedSongIds: ["song-abc", "song-def"],
      songs: [
        makeSong({ instrumental: true }),
        makeSong({ hash: "song-def", instrumental: true }),
      ],
    });

    const menu = commands.buildMenu(context());

    expect(item(menu, "library.markInstrumentalSelected:2").indicator).toBe(
      "checked",
    );
  });

  test("marks the instrumental toggle mixed when the selection disagrees", () => {
    setUp({
      selectedSongIds: ["song-abc", "song-def"],
      songs: [
        makeSong({ instrumental: true }),
        makeSong({ hash: "song-def", instrumental: false }),
      ],
    });

    const menu = commands.buildMenu(context());

    expect(item(menu, "library.markInstrumentalSelected:2").indicator).toBe(
      "mixed",
    );
  });

  test("checks no language when the selection spans several", () => {
    setUp({
      selectedSongIds: ["song-abc", "song-def"],
      songs: [
        makeSong({ language: "mandarin" }),
        makeSong({ hash: "song-def", language: "japanese" }),
      ],
    });

    const languages = submenu(
      commands.buildMenu(context()),
      "library.language",
    );

    expect(item(languages, "library.languageAuto").indicator).toBe("checked");
    expect(item(languages, "library.language_mandarin").indicator).toBeNull();
  });

  test("checks the shared language of the selection", () => {
    setUp({
      selectedSongIds: ["song-abc", "song-def"],
      songs: [
        makeSong({ language: "mandarin" }),
        makeSong({ hash: "song-def", language: "mandarin" }),
      ],
    });

    const languages = submenu(
      commands.buildMenu(context()),
      "library.language",
    );

    expect(item(languages, "library.language_mandarin").indicator).toBe(
      "checked",
    );
  });

  test("falls back to the row's own language when nothing is selected", () => {
    const languages = submenu(
      commands.buildMenu(context(makeSong({ language: "japanese" }))),
      "library.language",
    );

    expect(item(languages, "library.language_japanese").indicator).toBe(
      "checked",
    );
  });

  test("lists playlists with the membership of the whole context selection", () => {
    setUp({
      ...THREE_SELECTED,
      playlists: [
        { id: "pl-all", name: "Favorites" },
        { id: "pl-some", name: "Duets" },
        { id: "pl-none", name: "Empty" },
      ],
      playlistSongSets: new Map([
        ["pl-all", new Set(["song-abc", "song-def", "song-ghi"])],
        ["pl-some", new Set(["song-abc"])],
      ]),
    });

    const playlists = submenu(commands.buildMenu(context()), "playlist.addTo");

    expect(item(playlists, "Favorites").indicator).toBe("checked");
    expect(item(playlists, "Duets").indicator).toBe("mixed");
    expect(item(playlists, "Empty").indicator).toBeNull();
  });

  test("only offers removal from a playlist while one is active", () => {
    expect(labels(commands.buildMenu(context()))).not.toContain(
      "playlist.removeFromPlaylist",
    );

    setUp({ activePlaylistId: "pl-active" });

    expect(labels(commands.buildMenu(context()))).toContain(
      "playlist.removeFromPlaylist",
    );
  });
});

describe("buildMenu – items issue the command they name", () => {
  test("queues every context song once", () => {
    setUp(THREE_SELECTED);

    item(
      commands.buildMenu(context()),
      "library.queueAllSelected:3",
    ).onClick?.();

    expect(ports.calls).toEqual([
      { call: "queue.addToQueue", songId: "song-abc" },
      { call: "queue.addToQueue", songId: "song-def" },
      { call: "queue.addToQueue", songId: "song-ghi" },
    ]);
  });

  test("three songs selected issues one batch separation for the selection", async () => {
    setUp(THREE_SELECTED);

    item(
      commands.buildMenu(context()),
      "library.separateAllSelected:3",
    ).onClick?.();

    await vi.waitFor(() => {
      expect(mockBatchSeparate).toHaveBeenCalledWith([
        "song-abc",
        "song-def",
        "song-ghi",
      ]);
    });
    expect(mockBatchSeparate).toHaveBeenCalledOnce();
  });

  test("plays the row through the player", () => {
    item(commands.buildMenu(context()), "library.playNow").onClick?.();

    expect(ports.calls).toEqual([
      { call: "player.playNow", songId: "song-abc" },
    ]);
  });

  test("opens the row's edit, properties and delete dialogs", () => {
    const menu = commands.buildMenu(context());

    item(menu, "library.editInfo").onClick?.();
    item(menu, "library.properties").onClick?.();
    item(menu, "library.delete").onClick?.();

    expect(ports.calls).toEqual([
      { call: "dialogs.editInfo" },
      { call: "dialogs.properties" },
      { call: "dialogs.confirmDelete", songIds: ["song-abc"] },
    ]);
  });

  test("confirms deletion of the whole selection from the multi-select menu", () => {
    setUp(THREE_SELECTED);

    item(commands.buildMenu(context()), "library.deleteSelected:3").onClick?.();

    expect(ports.calls).toEqual([
      {
        call: "dialogs.confirmDelete",
        songIds: ["song-abc", "song-def", "song-ghi"],
      },
    ]);
  });

  test("asks the row for a new playlist instead of creating one itself", () => {
    const playlists = submenu(commands.buildMenu(context()), "playlist.addTo");

    item(playlists, "playlist.newPlaylist").onClick?.();

    expect(ports.calls).toEqual([{ call: "dialogs.createPlaylist" }]);
  });

  test("adds to a playlist the selection is absent from and removes from one it is in", async () => {
    setUp({
      playlists: [
        { id: "pl-in", name: "Favorites" },
        { id: "pl-out", name: "Duets" },
      ],
      playlistSongSets: new Map([["pl-in", new Set(["song-abc"])]]),
    });

    const playlists = submenu(commands.buildMenu(context()), "playlist.addTo");
    item(playlists, "Favorites").onClick?.();
    item(playlists, "Duets").onClick?.();

    await vi.waitFor(() => {
      expect(ports.names()).toEqual([
        "playlist.removeSongs",
        "playlist.addSongs",
      ]);
    });
  });
});

describe("execute – queue and rotation", () => {
  test("plays next without touching rotation while no singers are registered", async () => {
    await commands.execute({ id: "playNext" }, context());

    expect(ports.calls).toEqual([
      { call: "queue.playNext", songId: "song-abc" },
    ]);
  });

  test("plays next, assigns the next singer and advances the rotation", async () => {
    setUp({ singerNames: ["Alice", "Bob"], nextSinger: "Alice" });

    await commands.execute({ id: "playNext" }, context());

    expect(ports.calls).toEqual([
      { call: "queue.playNext", songId: "song-abc" },
      { call: "rotation.assignSinger", songId: "song-abc", singer: "Alice" },
      { call: "rotation.advance" },
    ]);
  });

  test("enqueues without touching rotation while no singers are registered", async () => {
    await commands.execute({ id: "addToQueue" }, context());

    expect(ports.calls).toEqual([
      { call: "queue.addToQueue", songId: "song-abc" },
    ]);
  });

  test("enqueues, assigns the next singer and advances the rotation", async () => {
    setUp({ singerNames: ["Charlie"], nextSinger: "Charlie" });

    await commands.execute({ id: "addToQueue" }, context());

    expect(ports.calls).toEqual([
      { call: "queue.addToQueue", songId: "song-abc" },
      { call: "rotation.assignSinger", songId: "song-abc", singer: "Charlie" },
      { call: "rotation.advance" },
    ]);
  });
});

describe("execute – library mutations", () => {
  test("toggles the instrumental flag on the capable songs only", async () => {
    setUp({
      selectedSongIds: ["song-abc", "song-def"],
      songs: [
        makeSong(),
        makeSong({ hash: "song-def", cdg_path: "/music/def.cdg" }),
      ],
    });

    await commands.execute({ id: "toggleInstrumental" }, context());

    expect(ports.calls).toEqual([
      {
        call: "library.setSongsInstrumental",
        songIds: ["song-abc"],
        instrumental: true,
      },
    ]);
  });

  test("clears the instrumental flag when the whole selection carries it", async () => {
    setUp({
      selectedSongIds: ["song-abc"],
      songs: [makeSong({ instrumental: true })],
    });

    await commands.execute({ id: "toggleInstrumental" }, context());

    expect(ports.calls).toEqual([
      {
        call: "library.setSongsInstrumental",
        songIds: ["song-abc"],
        instrumental: false,
      },
    ]);
  });

  test("applies a language to the row when nothing is selected", async () => {
    await commands.execute(
      { id: "setLanguage", language: "mandarin" },
      context(),
    );

    expect(ports.calls).toEqual([
      {
        call: "library.setSongsLanguage",
        songIds: ["song-abc"],
        language: "mandarin",
      },
    ]);
  });

  test("applies a language to the whole selection", async () => {
    setUp(THREE_SELECTED);

    await commands.execute({ id: "setLanguage", language: null }, context());

    expect(ports.calls).toEqual([
      {
        call: "library.setSongsLanguage",
        songIds: ["song-abc", "song-def", "song-ghi"],
        language: null,
      },
    ]);
  });

  test("applies a language to the row when the selection excludes it", async () => {
    setUp({
      selectedSongIds: ["song-def", "song-ghi"],
      songs: [makeSong(), makeSong({ hash: "song-def" })],
    });

    await commands.execute(
      { id: "setLanguage", language: "mandarin" },
      context(),
    );

    expect(ports.calls).toEqual([
      {
        call: "library.setSongsLanguage",
        songIds: ["song-abc"],
        language: "mandarin",
      },
    ]);
  });

  test("extracts cover art for the row alone or for the whole selection", async () => {
    setUp(THREE_SELECTED);

    await commands.execute({ id: "extractCoverArt" }, context());
    await commands.execute({ id: "extractSelectedCoverArt" }, context());

    expect(ports.calls).toEqual([
      { call: "library.extractEmbeddedCoverArt", songIds: ["song-abc"] },
      {
        call: "library.extractEmbeddedCoverArt",
        songIds: ["song-abc", "song-def", "song-ghi"],
      },
    ]);
  });

  test("reports a failed batch separation", async () => {
    const error = new Error("separate failed");
    mockBatchSeparate.mockRejectedValue(error);

    await commands.execute({ id: "separateSelected" }, context());

    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });
});

describe("execute – lyrics", () => {
  test("extracts embedded lyrics for the row", async () => {
    await commands.execute({ id: "extractEmbeddedLyrics" }, context());

    expect(mockExtractEmbeddedLyrics).toHaveBeenCalledWith("song-abc");
  });

  test("reloads the lyrics view when the fetched song is the one playing", async () => {
    setUp({ currentSongId: "song-abc" });
    mockFetchLyricsOnline.mockResolvedValue({ lines: [{ text: "hello" }] });

    await commands.execute({ id: "fetchLyricsOnline" }, context());

    expect(mockFetchLyricsOnline).toHaveBeenCalledWith(
      "song-abc",
      "user_replace",
    );
    expect(ports.calls).toEqual([
      { call: "lyrics.clear" },
      { call: "lyrics.fetchLyrics", songId: "song-abc" },
    ]);
  });

  test("leaves the lyrics view alone while another song is playing", async () => {
    setUp({ currentSongId: "other-song" });
    mockFetchLyricsOnline.mockResolvedValue({ lines: [{ text: "hello" }] });

    await commands.execute({ id: "fetchLyricsOnline" }, context());

    expect(ports.calls).toEqual([]);
  });

  test("leaves the lyrics view alone when the fetch returns no lines", async () => {
    setUp({ currentSongId: "song-abc" });
    mockFetchLyricsOnline.mockResolvedValue({ lines: [] });

    await commands.execute({ id: "fetchLyricsOnline" }, context());

    expect(ports.calls).toEqual([]);
  });

  test("reports a failed online lyrics fetch", async () => {
    const error = new Error("fetch failed");
    mockFetchLyricsOnline.mockRejectedValue(error);

    await commands.execute({ id: "fetchLyricsOnline" }, context());

    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });
});

describe("execute – playlists", () => {
  test("adds the whole context selection and reports how many songs moved", async () => {
    setUp(THREE_SELECTED);

    await commands.execute(
      { id: "addToPlaylist", playlistId: "pl-1" },
      context(),
    );

    expect(ports.calls).toEqual([
      {
        call: "playlist.addSongs",
        playlistId: "pl-1",
        songIds: ["song-abc", "song-def", "song-ghi"],
      },
    ]);
    expect(mockNotifySuccess).toHaveBeenCalledWith("playlist.addedToast:3");
  });

  test("reports a failed playlist addition instead of a toast", async () => {
    const error = new Error("add failed");
    ports.failOn("playlist.addSongs", error);

    await commands.execute(
      { id: "addToPlaylist", playlistId: "pl-1" },
      context(),
    );

    expect(mockNotifyError).toHaveBeenCalledWith(error);
    expect(mockNotifySuccess).not.toHaveBeenCalled();
  });

  test("removes the whole context selection from a playlist", async () => {
    await commands.execute(
      { id: "removeFromPlaylist", playlistId: "pl-1" },
      context(),
    );

    expect(ports.calls).toEqual([
      {
        call: "playlist.removeSongs",
        playlistId: "pl-1",
        songIds: ["song-abc"],
      },
    ]);
    expect(mockNotifySuccess).toHaveBeenCalledWith(
      "playlist.removedFromPlaylistToast:1",
    );
  });

  test("reports a failed playlist removal instead of a toast", async () => {
    const error = new Error("remove failed");
    ports.failOn("playlist.removeSongs", error);

    await commands.execute(
      { id: "removeFromPlaylist", playlistId: "pl-1" },
      context(),
    );

    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });

  test("removes only the row from the active playlist", async () => {
    setUp({ ...THREE_SELECTED, activePlaylistId: "pl-active" });

    await commands.execute({ id: "removeFromActivePlaylist" }, context());

    expect(ports.calls).toEqual([
      {
        call: "playlist.removeSongs",
        playlistId: "pl-active",
        songIds: ["song-abc"],
      },
    ]);
    expect(mockNotifySuccess).toHaveBeenCalledWith(
      "playlist.removedFromPlaylistToast:1",
    );
  });

  test("does nothing when no playlist is active", async () => {
    await commands.execute({ id: "removeFromActivePlaylist" }, context());

    expect(ports.calls).toEqual([]);
    expect(mockNotifySuccess).not.toHaveBeenCalled();
  });

  test("creates a playlist and fills it with the context selection", async () => {
    setUp({ ...THREE_SELECTED, createdPlaylistId: "pl-new" });

    await commands.execute(
      { id: "createPlaylistAndAdd", name: "  Duets  " },
      context(),
    );

    expect(ports.calls).toEqual([
      { call: "playlist.create", name: "Duets" },
      {
        call: "playlist.addSongs",
        playlistId: "pl-new",
        songIds: ["song-abc", "song-def", "song-ghi"],
      },
    ]);
    expect(mockNotifySuccess).toHaveBeenCalledWith(
      "playlist.createdAndAddedToast:3",
    );
  });

  test("reports a failed playlist creation without adding songs", async () => {
    const error = new Error("create failed");
    ports.failOn("playlist.create", error);

    await commands.execute(
      { id: "createPlaylistAndAdd", name: "Duets" },
      context(),
    );

    expect(ports.names()).toEqual(["playlist.create"]);
    expect(mockNotifyError).toHaveBeenCalledWith(error);
  });
});
