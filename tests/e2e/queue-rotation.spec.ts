import { test, expect } from "./fixtures/base-test";

/**
 * Queue and rotation management E2E tests.
 *
 * Verifies queue panel interactions (add/remove/reorder songs) and the
 * rotation (singer rotation) feature that sits inside the queue panel.
 */
test.describe("Queue panel", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();
  });

  test("queue panel toggles visibility", async ({ page }) => {
    // Find the queue toggle button
    const queueButton = page.getByRole("button", { name: /queue/i });
    await expect(queueButton).toBeVisible();

    // Open the queue panel
    await queueButton.click();
    await expect(page.getByText(/up next/i)).toBeVisible({ timeout: 5000 });

    // Close the queue panel
    await queueButton.click();
  });

  test("empty queue shows empty state message", async ({ page }) => {
    const queueButton = page.getByRole("button", { name: /queue/i });
    await expect(queueButton).toBeVisible();

    await queueButton.click();

    // Empty queue should show an empty state
    // The i18n key "queue.empty" renders "No songs in queue" or similar
    await expect(
      page.getByText(/no songs|queue.*empty|empty/i).first(),
    ).toBeVisible({
      timeout: 5000,
    });
  });

  test("right-clicking a song offers queue actions", async ({ page }) => {
    // Right-click on a song in the library to get the context menu
    await page.getByText("Hotel California").click({ button: "right" });

    // Context menu should appear with queue-related options
    await expect(page.getByText(/add to queue|play next/i).first()).toBeVisible(
      { timeout: 5000 },
    );

    // Dismiss context menu
    await page.keyboard.press("Escape");
  });
});

test.describe("Rotation / singer management", () => {
  test("rotation controls are visible in the queue panel", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();

    // Open queue panel
    const queueButton = page.getByRole("button", { name: /queue/i });
    await expect(queueButton).toBeVisible();
    await queueButton.click();

    // Rotation section should show "Singer" label and an "Add Singer" button
    await expect(page.getByText(/singer/i).first()).toBeVisible({
      timeout: 5000,
    });
    await expect(page.getByText(/add singer/i).first()).toBeVisible();
  });

  test("add singer input can be opened", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Bohemian Rhapsody")).toBeVisible();

    const queueButton = page.getByRole("button", { name: /queue/i });
    await queueButton.click();
    await expect(page.getByText(/singer/i).first()).toBeVisible({
      timeout: 5000,
    });

    // Click "Add Singer"
    await page
      .getByText(/add singer/i)
      .first()
      .click();

    // Input field should appear
    const singerInput = page.getByRole("textbox", { name: /singer name/i });
    await expect(singerInput).toBeVisible();

    // Type a name and press Enter
    await singerInput.fill("Alice");
    await singerInput.press("Enter");

    // Singer tag should appear
    await expect(page.getByText("Alice")).toBeVisible();
  });
});
