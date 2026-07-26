// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

const { mockGetCoverArt, mockGetCoverArtThumbnail } = vi.hoisted(() => ({
  mockGetCoverArt: vi.fn(),
  mockGetCoverArtThumbnail: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => `asset://localhost/${encodeURI(path)}`,
}));

vi.mock("@/lib/tauri/library", () => ({
  getCoverArt: mockGetCoverArt,
  getCoverArtThumbnail: mockGetCoverArtThumbnail,
}));

const { CoverArtThumbnail } = await import("./CoverArtThumbnail");

let container: HTMLDivElement;
let unmount: () => void;

beforeEach(() => {
  vi.clearAllMocks();
  mockGetCoverArtThumbnail.mockResolvedValue(null);
  mockGetCoverArt.mockResolvedValue(null);
  container = document.createElement("div");
  document.body.appendChild(container);
});

afterEach(() => {
  act(() => unmount?.());
  container.remove();
});

function render(node: React.ReactNode) {
  const root = createRoot(container);
  unmount = () => root.unmount();
  act(() => root.render(node));
}

function image(): HTMLImageElement | null {
  return container.querySelector("img");
}

describe("CoverArtThumbnail asset protocol", () => {
  test("serves the derivative off disk and skips the IPC read entirely", () => {
    render(
      <CoverArtThumbnail
        songHash="song-1"
        coverArt={null}
        thumbnailPath="/lib/artwork/thumb_abc_80.webp"
        alt="cover"
      />,
    );

    expect(image()?.getAttribute("src")).toBe(
      "asset://localhost//lib/artwork/thumb_abc_80.webp",
    );
    expect(mockGetCoverArtThumbnail).not.toHaveBeenCalled();
    expect(mockGetCoverArt).not.toHaveBeenCalled();
  });

  test("falls back to the IPC read when the derivative file will not load", async () => {
    // The IPC command is also what triggers the Rust-side lazy regeneration,
    // so a derivative deleted from disk self-heals through this path.
    mockGetCoverArtThumbnail.mockResolvedValue([0xff, 0xd8, 0x00]);

    render(
      <CoverArtThumbnail
        songHash="song-1"
        coverArt={null}
        thumbnailPath="/lib/artwork/thumb_gone_80.webp"
        alt="cover"
      />,
    );

    await act(async () => {
      image()!.dispatchEvent(new Event("error", { bubbles: false }));
    });

    expect(mockGetCoverArtThumbnail).toHaveBeenCalledWith("song-1");
  });

  test("reads over IPC when the song has no recorded derivative", async () => {
    mockGetCoverArtThumbnail.mockResolvedValue([0xff, 0xd8, 0x00]);

    await act(async () => {
      render(
        <CoverArtThumbnail songHash="song-2" coverArt={null} alt="cover" />,
      );
    });

    expect(mockGetCoverArtThumbnail).toHaveBeenCalledWith("song-2");
  });
});
