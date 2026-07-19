// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { resetCoverArtCacheForTests } from "@/lib/cover-art";
import type { CoverArtBytes } from "@/types/ipc";

const { mockGetCoverArtThumbnail, mockGetCoverArt } = vi.hoisted(() => ({
  mockGetCoverArtThumbnail: vi.fn<() => Promise<CoverArtBytes>>(),
  mockGetCoverArt: vi.fn<() => Promise<CoverArtBytes>>(),
}));

vi.mock("@/lib/tauri/library", () => ({
  getCoverArtThumbnail: mockGetCoverArtThumbnail,
  getCoverArt: mockGetCoverArt,
}));

vi.mock("@/lib/cover-art", async () => {
  const actual =
    await vi.importActual<typeof import("@/lib/cover-art")>("@/lib/cover-art");
  return {
    ...actual,
    useCoverArtUrl: vi.fn(
      (_hash: string, bytes: CoverArtBytes, _size: string) =>
        bytes ? "blob:cover" : null,
    ),
  };
});

import { CoverArtThumbnail } from "./CoverArtThumbnail";

describe("CoverArtThumbnail async fetch", () => {
  let container: HTMLElement;
  let root: ReturnType<typeof createRoot> | null;

  beforeEach(() => {
    vi.stubGlobal("URL", {
      createObjectURL: vi.fn(() => "blob:cover"),
      revokeObjectURL: vi.fn(),
    });
    container = document.createElement("div");
    document.body.appendChild(container);
    root = null;
    mockGetCoverArtThumbnail.mockReset();
    mockGetCoverArt.mockReset();
  });

  afterEach(() => {
    if (root) {
      act(() => {
        root!.unmount();
      });
    }
    resetCoverArtCacheForTests();
    container.remove();
    vi.unstubAllGlobals();
  });

  function render(node: React.ReactNode) {
    root = createRoot(container);
    act(() => {
      root!.render(node);
    });
  }

  async function renderAsync(node: React.ReactNode) {
    root = createRoot(container);
    await act(async () => {
      root!.render(node);
    });
  }

  test("fetches thumbnail derivative first when coverArt is not provided", async () => {
    mockGetCoverArtThumbnail.mockResolvedValueOnce([0x52, 0x49, 0x46, 0x46]);

    await renderAsync(
      <CoverArtThumbnail
        songHash="song-fetch-thumb"
        coverArt={null}
        alt="Test"
        className="h-11 w-11"
      />,
    );

    expect(mockGetCoverArtThumbnail).toHaveBeenCalledWith("song-fetch-thumb");
    expect(mockGetCoverArt).not.toHaveBeenCalled();
  });

  test("falls back to full cover art when thumbnail is null", async () => {
    mockGetCoverArtThumbnail.mockResolvedValueOnce(null);
    mockGetCoverArt.mockResolvedValueOnce([0xff, 0xd8, 0x00]);

    await renderAsync(
      <CoverArtThumbnail
        songHash="song-fetch-fallback"
        coverArt={null}
        alt="Test"
        className="h-11 w-11"
      />,
    );

    expect(mockGetCoverArtThumbnail).toHaveBeenCalledWith(
      "song-fetch-fallback",
    );
    expect(mockGetCoverArt).toHaveBeenCalledWith("song-fetch-fallback");
  });

  test("does not fetch when coverArt bytes are already provided", async () => {
    render(
      <CoverArtThumbnail
        songHash="song-has-bytes"
        coverArt={[0xff, 0xd8, 0x00]}
        alt="Test"
        className="h-11 w-11"
      />,
    );

    expect(mockGetCoverArtThumbnail).not.toHaveBeenCalled();
    expect(mockGetCoverArt).not.toHaveBeenCalled();
  });

  test("renders placeholder when both thumbnail and full fetch fail", async () => {
    mockGetCoverArtThumbnail.mockRejectedValueOnce(new Error("network"));
    mockGetCoverArt.mockRejectedValueOnce(new Error("network"));

    await renderAsync(
      <CoverArtThumbnail
        songHash="song-fetch-error"
        coverArt={null}
        alt="Test"
        className="h-11 w-11"
      />,
    );

    // The placeholder dot should be rendered (no img element).
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector('[aria-hidden="true"]')).not.toBeNull();
  });
});
