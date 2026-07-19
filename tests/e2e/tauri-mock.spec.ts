import { expect, objectRecord, test } from "./fixtures/base-test";

test.describe("Tauri IPC mock contract", () => {
  test("fails fast for unhandled invoke commands", async ({ page }) => {
    await page.goto("/");

    await expect(
      page.evaluate(() =>
        window.__TAURI_INTERNALS__.invoke("unknown_test_command", {}),
      ),
    ).rejects.toThrow("Unhandled Tauri invoke in mock");
  });

  test("set_volume preserves volume=0 via nullish handling", async ({
    page,
  }) => {
    await page.goto("/");

    const result = await page.evaluate(async () => {
      const r1 = await window.__TAURI_INTERNALS__.invoke("set_volume", {
        level: 0,
      });
      const r2 = await window.__TAURI_INTERNALS__.invoke("get_playback_state");
      return { set: r1, get: r2 };
    });

    expect(objectRecord(result.set)?.volume).toBe(0);
    expect(objectRecord(result.get)?.volume).toBe(0);
  });

  test("set_stem_volume updates the named stem and preserves has_stems", async ({
    page,
  }) => {
    await page.goto("/");

    // Seed has_stems + stem_mode via setPlaybackSnapshot
    await page.evaluate(() =>
      window.__OPENKARA_E2E__.setPlaybackSnapshot({
        has_stems: true,
        stem_mode: "two_stem",
      }),
    );

    const result = await page.evaluate(async () => {
      const r = await window.__TAURI_INTERNALS__.invoke("set_stem_volume", {
        stem: "vocals",
        level: 0,
      });
      return r;
    });

    const record = objectRecord(result);
    expect(record?.has_stems).toBe(true);
    expect(record?.stem_mode).toBe("two_stem");
    expect(objectRecord(record?.stem_volumes)?.vocals).toBe(0);
  });

  test("setPlaybackSnapshot merges nested stem_volumes per-stem", async ({
    page,
  }) => {
    await page.goto("/");

    const result = await page.evaluate(() =>
      window.__OPENKARA_E2E__.setPlaybackSnapshot({
        stem_volumes: { vocals: 0.3, drums: 0.7 },
      }),
    );

    const stemVolumes = objectRecord(result)?.stem_volumes;
    expect(objectRecord(stemVolumes)?.vocals).toBe(0.3);
    expect(objectRecord(stemVolumes)?.drums).toBe(0.7);
    // Untouched stems keep their defaults
    expect(objectRecord(stemVolumes)?.bass).toBe(1);
  });

  test("setSeparationCompleted marks a song as completed", async ({ page }) => {
    await page.goto("/");

    const statuses = await page.evaluate(async () => {
      window.__OPENKARA_E2E__.setSeparationCompleted("earfquake");
      return await window.__TAURI_INTERNALS__.invoke(
        "get_all_separation_statuses",
      );
    });

    expect(Array.isArray(statuses)).toBe(true);
    expect(statuses).toContainEqual(
      expect.objectContaining({ song_id: "earfquake", state: "completed" }),
    );
  });
});

declare global {
  interface Window {
    __TAURI_INTERNALS__: {
      invoke: (cmd: string, args?: unknown) => Promise<unknown>;
    };
  }
}
