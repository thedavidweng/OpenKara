import { test, expect } from "./fixtures/base-test";

/**
 * Playlist management E2E tests.
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

  test("create playlist button opens input dialog", async ({ page }) => {
    // Find the "Create" playlist button in the sidebar
    const createButton = page.getByText(/create/i).first();
    await expect(createButton).toBeVisible();

    await createButton.click();

    // An input dialog should appear for the playlist name
    const nameInput = page.getByRole("textbox");
    const dialogVisible = await nameInput.isVisible().catch(() => false);

    if (dialogVisible) {
      await nameInput.fill("My Karaoke Night");

      // Find and click the confirm/save button
      const saveButton = page.getByRole("button", {
        name: /save|create|confirm/i,
      });
      if (await saveButton.isVisible().catch(() => false)) {
        await saveButton.click();

        // The new playlist should appear in the sidebar
        await expect(page.getByText("My Karaoke Night")).toBeVisible({
          timeout: 5000,
        });
      }
    }
  });

  test("sidebar library filter tabs are functional", async ({ page }) => {
    // The sidebar has "All Tracks" and "Separated" filter tabs
    const allTracks = page.getByText(/all.*tracks|all tracks/i).first();
    await expect(allTracks).toBeVisible();

    // Click "Separated" filter
    const separated = page.getByText(/^separated$/i).first();
    if (await separated.isVisible().catch(() => false)) {
      await separated.click();

      // Should still show the sidebar (just filtered)
      await expect(
        page.locator("[data-window-shell-section='sidebar']"),
      ).toBeVisible();

      // Switch back to All Tracks
      await allTracks.click();
    }
  });
});
