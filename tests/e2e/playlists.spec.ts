import { expect, objectRecord, test } from "./fixtures/base-test";

/**
 * Playlist management UI smoke tests.
 *
 * Verifies creating playlists from the sidebar and navigating between
 * playlist and library views.
 */
test.describe("Playlist management", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();
  });

  test("sidebar shows playlist section", async ({ page }) => {
    // The sidebar should contain a "Playlists" section header
    await expect(page.getByText(/playlist/i).first()).toBeVisible();
  });

  test("empty playlist section shows empty message", async ({ page }) => {
    // When no playlists exist, should show an empty state
    // i18n key: "playlist.empty"
    await expect(page.getByText(/no playlists|empty/i).first()).toBeVisible();
  });

  test("creating a playlist persists through the mocked IPC state", async ({
    page,
    tauriMock,
  }) => {
    // Find the "Create" playlist button in the sidebar
    const createButton = page.getByRole("button", { name: "+ New Playlist" });
    await expect(createButton).toBeVisible();

    await createButton.click();

    // An input dialog should appear for the playlist name
    const nameInput = page.getByRole("textbox", { name: "New Playlist" });
    await expect(nameInput).toBeVisible();
    await nameInput.fill("My Karaoke Night");

    await page.getByRole("button", { name: "Save" }).click();

    // The new playlist should appear in the sidebar after the store reloads
    await expect(page.getByText("My Karaoke Night")).toBeVisible({
      timeout: 5000,
    });

    await expect
      .poll(async () =>
        (await tauriMock.getInvokeCalls()).some((call) => {
          const args = objectRecord(call.args);
          return (
            call.cmd === "create_playlist" && args?.name === "My Karaoke Night"
          );
        }),
      )
      .toBe(true);

    await page.getByText("My Karaoke Night").click();
    await expect(page.getByText("My Karaoke Night").first()).toBeVisible();
    await expect(page.getByText("Bohemian Rhapsody")).not.toBeVisible();
  });

  test("sidebar library filter tabs are functional", async ({ page }) => {
    // The sidebar has "All Tracks" and "Separated" filter tabs
    const allTracks = page.getByText(/all.*tracks|all tracks/i).first();
    await expect(allTracks).toBeVisible();

    // Click "Separated" filter
    const separated = page.getByText(/^separated$/i).first();
    await expect(separated).toBeVisible();
    await separated.click();

    // Should still show the sidebar (just filtered)
    await expect(
      page.locator("[data-window-shell-section='sidebar']"),
    ).toBeVisible();

    // Switch back to All Tracks
    await allTracks.click();
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();
  });
});
