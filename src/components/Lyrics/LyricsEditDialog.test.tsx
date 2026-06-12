import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { LyricsEditDialog } from "./LyricsEditDialog";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/stores/lyrics-store", () => ({
  useLyricsStore: {
    getState: () => ({
      saveManualLyrics: vi.fn(),
    }),
  },
}));

describe("LyricsEditDialog format detection", () => {
  test("does not classify LRC timestamps as LYS", () => {
    const markup = renderToStaticMarkup(
      <LyricsEditDialog
        open
        onClose={vi.fn()}
        songId="song-1"
        existingLyrics="[00:12.34]Hello"
      />,
    );

    expect(markup).toContain("lyrics.detectedLrc");
    expect(markup).not.toContain("lyrics.detectedLys");
  });

  test("classifies bracketed single-digit LYS lines as LYS", () => {
    const markup = renderToStaticMarkup(
      <LyricsEditDialog
        open
        onClose={vi.fn()}
        songId="song-1"
        existingLyrics="[0]\n[100,500]Hello"
      />,
    );

    expect(markup).toContain("lyrics.detectedLys");
  });
});
