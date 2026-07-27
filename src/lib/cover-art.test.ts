import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import {
  detectCoverArtMime,
  invalidateCoverArtUrl,
  releaseCoverArtUrl,
  resetCoverArtCacheForTests,
  retainCoverArtUrl,
} from "./cover-art";

describe("cover-art", () => {
  beforeEach(() => {
    vi.stubGlobal("URL", {
      createObjectURL: vi.fn((blob: Blob) => `blob:${blob.type}`),
      revokeObjectURL: vi.fn(),
    });
  });

  afterEach(() => {
    resetCoverArtCacheForTests();
    vi.unstubAllGlobals();
  });

  test("detects common cover art mime types", () => {
    expect(detectCoverArtMime([0xff, 0xd8, 0x00])).toBe("image/jpeg");
    expect(detectCoverArtMime([0x89, 0x50, 0x4e, 0x47])).toBe("image/png");
    expect(detectCoverArtMime([0x47, 0x49, 0x46])).toBe("image/gif");
    expect(detectCoverArtMime([0x52, 0x49, 0x46, 0x46])).toBe("image/webp");
  });

  test("defaults to image/jpeg for empty or null bytes", () => {
    expect(detectCoverArtMime([])).toBe("image/jpeg");
    expect(detectCoverArtMime(null as unknown as number[])).toBe("image/jpeg");
  });

  test("defaults to image/jpeg for unknown magic bytes", () => {
    expect(detectCoverArtMime([0x00, 0x01, 0x02, 0x03])).toBe("image/jpeg");
  });

  test("reuses cached object urls per song hash and revokes after the final release", () => {
    const jpegBytes = [0xff, 0xd8, 0x00];

    const first = retainCoverArtUrl("song-1", jpegBytes);
    const second = retainCoverArtUrl("song-1", jpegBytes);

    expect(first).toBe("blob:image/jpeg");
    expect(second).toBe(first);
    expect(URL.createObjectURL).toHaveBeenCalledTimes(1);

    releaseCoverArtUrl("song-1");
    expect(URL.revokeObjectURL).not.toHaveBeenCalled();

    releaseCoverArtUrl("song-1");
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:image/jpeg");
  });

  test("invalidates a cached object url so a refreshed cover can replace it immediately", () => {
    const first = retainCoverArtUrl("song-1", [0xff, 0xd8, 0x00]);

    expect(first).toBe("blob:image/jpeg");
    expect(URL.createObjectURL).toHaveBeenCalledTimes(1);

    invalidateCoverArtUrl("song-1");

    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:image/jpeg");

    const second = retainCoverArtUrl("song-1", [0x89, 0x50, 0x4e, 0x47]);

    expect(second).toBe("blob:image/png");
    expect(URL.createObjectURL).toHaveBeenCalledTimes(2);
  });

  test("accepts Uint8Array cover bytes from the Tauri IPC bridge", () => {
    const url = retainCoverArtUrl(
      "song-typed-array",
      new Uint8Array([0xff, 0xd8, 0x00]),
    );

    expect(url).toBe("blob:image/jpeg");
    expect(URL.createObjectURL).toHaveBeenCalledTimes(1);
  });

  test("accepts ArrayBuffer cover bytes from the Tauri IPC bridge", () => {
    const bytes = new Uint8Array([0x89, 0x50, 0x4e, 0x47]).buffer;
    const url = retainCoverArtUrl("song-array-buffer", bytes);

    expect(url).toBe("blob:image/png");
    expect(URL.createObjectURL).toHaveBeenCalledTimes(1);
  });

  test("returns null from retainCoverArtUrl when bytes are empty", () => {
    expect(retainCoverArtUrl("song-empty", [])).toBeNull();
  });

  test("releaseCoverArtUrl is a no-op for unknown song hashes", () => {
    releaseCoverArtUrl("nonexistent-hash");
    expect(URL.revokeObjectURL).not.toHaveBeenCalled();
  });

  test("invalidateCoverArtUrl is a no-op for unknown song hashes", () => {
    invalidateCoverArtUrl("nonexistent-hash");
    expect(URL.revokeObjectURL).not.toHaveBeenCalled();
  });

  test("maintains independent ref counts per size for the same song", () => {
    const jpeg = [0xff, 0xd8, 0x00];
    const png = [0x89, 0x50, 0x4e, 0x47];

    const thumbUrl1 = retainCoverArtUrl("song-size", jpeg, "thumb");
    retainCoverArtUrl("song-size", jpeg, "thumb"); // second retain for refcount=2
    const previewUrl = retainCoverArtUrl("song-size", png, "preview");

    expect(thumbUrl1).toBe("blob:image/jpeg");
    expect(previewUrl).toBe("blob:image/png");
    expect(thumbUrl1).not.toBe(previewUrl);

    // Releasing one thumb ref (refcount 2→1) does not revoke either URL.
    releaseCoverArtUrl("song-size", "thumb");
    expect(URL.revokeObjectURL).not.toHaveBeenCalledWith(thumbUrl1);
    expect(URL.revokeObjectURL).not.toHaveBeenCalledWith(previewUrl);

    // Final thumb release revokes the thumb URL but not the preview URL.
    releaseCoverArtUrl("song-size", "thumb");
    expect(URL.revokeObjectURL).toHaveBeenCalledWith(thumbUrl1);
    expect(URL.revokeObjectURL).not.toHaveBeenCalledWith(previewUrl);

    // Final preview release revokes the preview URL.
    releaseCoverArtUrl("song-size", "preview");
    expect(URL.revokeObjectURL).toHaveBeenCalledWith(previewUrl);
  });

  test("same-key byte replacement revokes the old URL and returns a new one", () => {
    const jpeg = [0xff, 0xd8, 0x00];
    const png = [0x89, 0x50, 0x4e, 0x47];

    const first = retainCoverArtUrl("song-replace", jpeg, "thumb");
    expect(first).toBe("blob:image/jpeg");

    // New bytes under the same key → old URL revoked, new URL created.
    const second = retainCoverArtUrl("song-replace", png, "thumb");
    expect(second).toBe("blob:image/png");
    expect(second).not.toBe(first);
    expect(URL.revokeObjectURL).toHaveBeenCalledWith(first);

    // Releasing the replacement cleans up the new URL.
    releaseCoverArtUrl("song-replace", "thumb");
    expect(URL.revokeObjectURL).toHaveBeenCalledWith(second);
  });

  test("releaseCoverArtUrl with url guard skips stale cleanups after byte replacement", () => {
    const jpeg = [0xff, 0xd8, 0x00];
    const png = [0x89, 0x50, 0x4e, 0x47];

    const urlA = retainCoverArtUrl("song-guard", jpeg, "thumb");
    const urlB = retainCoverArtUrl("song-guard", png, "thumb");
    expect(urlA).not.toBe(urlB);

    // Old cleanup fires with the stale url — must be a no-op.
    releaseCoverArtUrl("song-guard", "thumb", urlA);
    expect(URL.revokeObjectURL).not.toHaveBeenCalledWith(urlB);

    // New cleanup fires with the current url — releases normally.
    releaseCoverArtUrl("song-guard", "thumb", urlB);
    expect(URL.revokeObjectURL).toHaveBeenCalledWith(urlB);
  });

  test("same-key identical bytes reuse the cached URL without revoking", () => {
    const jpeg = [0xff, 0xd8, 0x00];

    const first = retainCoverArtUrl("song-same", jpeg, "thumb");
    const second = retainCoverArtUrl("song-same", jpeg, "thumb");

    expect(first).toBe(second);
    expect(URL.createObjectURL).toHaveBeenCalledTimes(1);
    expect(URL.revokeObjectURL).not.toHaveBeenCalled();
  });

  test("invalidateCoverArtUrl revokes every size variant for a song", () => {
    const jpeg = [0xff, 0xd8, 0x00];
    const png = [0x89, 0x50, 0x4e, 0x47];

    const thumbUrl = retainCoverArtUrl("song-multi", jpeg, "thumb");
    const previewUrl = retainCoverArtUrl("song-multi", png, "preview");
    const originalUrl = retainCoverArtUrl("song-multi", jpeg, "original");

    invalidateCoverArtUrl("song-multi");

    expect(URL.revokeObjectURL).toHaveBeenCalledWith(thumbUrl);
    expect(URL.revokeObjectURL).toHaveBeenCalledWith(previewUrl);
    expect(URL.revokeObjectURL).toHaveBeenCalledWith(originalUrl);
  });

  test("releaseCoverArtUrl with explicit size only releases that size", () => {
    const jpeg = [0xff, 0xd8, 0x00];

    retainCoverArtUrl("song-release-size", jpeg, "thumb");
    retainCoverArtUrl("song-release-size", jpeg, "original");

    releaseCoverArtUrl("song-release-size", "thumb");
    // Only the thumb URL should be revoked; original still held.
    expect(URL.revokeObjectURL).toHaveBeenCalledTimes(1);

    releaseCoverArtUrl("song-release-size", "original");
    expect(URL.revokeObjectURL).toHaveBeenCalledTimes(2);
  });
});
