import { expect, objectRecord, test } from "./fixtures/base-test";

/**
 * Queue and rotation management UI smoke tests.
 *
 * Verifies queue panel interactions (add/remove/reorder songs) and the
 * rotation (singer rotation) feature that sits inside the queue panel.
 */
test.describe("Queue panel", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
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

  test("right-clicking a song exposes native queue actions that update the queue", async ({
    page,
    tauriMock,
  }) => {
    // Right-click on a song in the library to get the context menu
    await page
      .getByRole("button", { name: "See You Again" })
      .click({ button: "right" });

    await expect
      .poll(async () => tauriMock.getLastNativeMenu())
      .toMatchObject({
        items: expect.arrayContaining([
          expect.objectContaining({ label: "Play Next" }),
          expect.objectContaining({ label: "Add to Queue" }),
        ]),
      });

    await tauriMock.clickNativeMenuItem("Add to Queue");

    const queueButton = page.getByRole("button", { name: /queue/i });
    await queueButton.click();
    await expect(
      page.getByTestId("queue-panel").getByText("See You Again"),
    ).toBeVisible({ timeout: 5000 });
  });
});

test.describe("Rotation / singer management", () => {
  test("rotation controls are visible in the queue panel", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();

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

  test("add singer input persists through the rotation IPC state", async ({
    page,
    tauriMock,
  }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();

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

    await expect
      .poll(async () =>
        (await tauriMock.getInvokeCalls()).some((call) => {
          const args = objectRecord(call.args);
          const rotation = objectRecord(args?.rotation);
          const singers = rotation?.singer_names;
          return (
            call.cmd === "set_rotation_state" &&
            Array.isArray(singers) &&
            singers.includes("Alice")
          );
        }),
      )
      .toBe(true);
  });
});
