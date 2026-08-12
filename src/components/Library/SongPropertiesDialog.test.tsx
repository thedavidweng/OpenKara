// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { BackendProvider } from "@/lib/backend";
import { createMockBackend } from "@/lib/backend/mock-backend";
import { SongPropertiesDialog } from "./SongPropertiesDialog";
import { useLibraryStore } from "@/stores/library-store";
import type { Song } from "@/types/ipc";

const mockGetSongProperties = vi.fn();
const backend = createMockBackend({
  overrides: { library: { getSongProperties: mockGetSongProperties } },
});

vi.mock("react-i18next", () => ({
  initReactI18next: {
    type: "3rdParty",
    init: () => {},
  },
  useTranslation: () => ({ t: (key: string) => key }),
}));

const song: Song = {
  hash: "song-properties-test",
  file_path: "media/song.mp3",
  audio_source_kind: "original",
  cdg_path: null,
  media_g_container: null,
  instrumental: false,
  language: null,
  title: "Test song",
  artist: null,
  album: null,
  duration_ms: 1000,
  cover_art: null,
  has_cover_art: false,
  artwork_thumb_path: null,
  imported_at: 0,
  original_ext: "mp3",
};

describe("SongPropertiesDialog", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    mockGetSongProperties.mockResolvedValue({
      format: "MP3",
      sample_rate: 44_100,
      channels: 2,
      bit_rate: 320,
      file_size: 1,
      duration_ms: 1000,
      hash: song.hash,
    });
    useLibraryStore.setState({ songs: [], separationStatuses: {} });
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  test("exposes its native overlay as a labelled modal dialog", () => {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);

    act(() => {
      root.render(
        <BackendProvider backend={backend}>
          <SongPropertiesDialog song={song} onClose={() => {}} />
        </BackendProvider>,
      );
    });

    const dialog = document.querySelector('[role="dialog"]');
    expect(dialog?.getAttribute("aria-modal")).toBe("true");
    expect(dialog?.getAttribute("aria-labelledby")).toBe(
      `song-properties-heading-${song.hash}`,
    );
    expect(
      document.querySelector('button[aria-label="common.close"]'),
    ).not.toBeNull();
  });
});
