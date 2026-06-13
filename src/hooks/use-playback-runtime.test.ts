import { describe, expect, test } from "vitest";

describe("use-playback-runtime wiring", () => {
  test("registers upload progress listeners alongside separation listeners", async () => {
    const { default: src } = await import("./use-playback-runtime.ts?raw");

    expect(src).toContain("upload-progress");
    expect(src).toContain("upload-complete");
    expect(src).toContain("upload-error");
    expect(src).toContain("updateUploadStatus");
    expect(src).toContain("clearUploadStatus");
    expect(src).toContain("separation-progress");
  });

  test("applies playback-position snapshots directly without a state refresh fallback", async () => {
    const { default: src } = await import("./use-playback-runtime.ts?raw");

    expect(src).toContain("applyPlaybackPositionEvent");
    expect(src).not.toContain("getPlaybackState: api.getPlaybackState");
  });
});

// ─── F3: Playback-position listener leak ────────────────────

describe("F3: playback-position listener cleanup when unmount races listen()", () => {
  test("setup function checks cancelled flag after await listen (RED)", async () => {
    // The fix for F3 requires: after `await listen(...)`, check
    // `if (cancelled) { unlisten(); return; }` so the listener is
    // cleaned up even if unmount happened before listen resolved.
    const { default: src } = await import("./use-playback-runtime.ts?raw");

    // Extract the setup function body after the listen call
    // The pattern should be:
    //   unlisten = await listen(...)
    //   if (cancelled) { unlisten(); return; }
    const setupMatch = src.match(
      /unlisten\s*=\s*await\s+listen[\s\S]*?\n\s*\}\s*\n/,
    );
    expect(setupMatch).not.toBeNull();

    // After the listen assignment, there must be a cancelled check
    // before the setup function closes
    const afterListen = src.slice(
      src.indexOf("unlisten = await listen"),
      src.indexOf("void setup()"),
    );
    expect(afterListen).toContain("if (cancelled)");
    expect(afterListen).toContain("unlisten()");
  });
});
