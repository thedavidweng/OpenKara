import { expect, objectRecord, test } from "./fixtures/base-test";

/**
 * Playback controls UI smoke tests.
 *
 * Verifies play/pause, skip, and seek bar interactions against the
 * mocked Tauri backend.  The mock returns deterministic playback state
 * snapshots so we can assert UI transitions.
 */
test.describe("Playback controls", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
  });

  test("double-clicking a song starts playback", async ({
    page,
    tauriMock,
  }) => {
    await page.getByRole("button", { name: "Earfquake" }).dblclick();

    // The play button should become a pause button
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });

    await expect
      .poll(async () =>
        (await tauriMock.getInvokeCalls()).some((call) => {
          const args = objectRecord(call.args);
          return call.cmd === "play" && args?.songId === "earfquake";
        }),
      )
      .toBe(true);
  });

  test("pause button stops playback", async ({ page, tauriMock }) => {
    // Start playback
    await page.getByRole("button", { name: "Earfquake" }).dblclick();
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });

    // Pause
    await page.getByRole("button", { name: /pause/i }).click();

    // Should show play button again
    await expect(
      page.getByRole("button", { exact: true, name: "Play" }),
    ).toBeVisible({ timeout: 5000 });

    await expect
      .poll(async () =>
        (await tauriMock.getInvokeCalls()).some((call) => call.cmd === "pause"),
      )
      .toBe(true);
  });

  test("skip forward and back buttons exist and are clickable", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "Earfquake" }).dblclick();
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });

    // Skip forward
    const skipForward = page.getByRole("button", { name: /next/i });
    await expect(skipForward).toBeVisible();
    await skipForward.click();

    // Skip back
    const skipBack = page.getByRole("button", { name: /previous/i });
    await expect(skipBack).toBeVisible();
    await skipBack.click();
  });

  test("seek bar sends seek commands during playback", async ({
    page,
    tauriMock,
  }) => {
    await page.getByRole("button", { name: "Earfquake" }).dblclick();
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });

    // The seek bar should be rendered as a slider
    const seekBar = page.getByRole("slider", { name: /seek/i });
    await expect(seekBar).toBeVisible();
    const box = await seekBar.boundingBox();
    expect(box).not.toBeNull();
    if (box === null) {
      throw new Error("Seek slider has no bounding box");
    }
    await page.mouse.move(box.x + box.width * 0.25, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width * 0.5, box.y + box.height / 2);
    await page.mouse.up();

    await expect
      .poll(async () =>
        (await tauriMock.getInvokeCalls()).some((call) => call.cmd === "seek"),
      )
      .toBe(true);
  });

  test("now-playing info shows song title during playback", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "Earfquake" }).dblclick();

    // The playback bar area should show the currently playing song title
    // There may be multiple instances (sidebar + now-playing), so use .first()
    await expect(page.getByText("Earfquake").first()).toBeVisible();
  });

  test("keyboard-only activation selects, plays, and seeks", async ({
    page,
    tauriMock,
  }) => {
    const song = page.getByRole("button", { name: "Earfquake" });
    await song.focus();
    await page.keyboard.press("Space");
    await expect(song).toHaveAttribute("aria-pressed", "true");

    await page.keyboard.press("Enter");
    await expect(page.getByRole("button", { name: /pause/i })).toBeVisible({
      timeout: 5000,
    });

    const seekBar = page.getByRole("slider", { name: /seek/i });
    await seekBar.focus();
    await page.keyboard.press("ArrowRight");

    await expect
      .poll(async () =>
        (await tauriMock.getInvokeCalls()).some((call) => call.cmd === "seek"),
      )
      .toBe(true);
  });
});
