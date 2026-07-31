import { expect, test } from "./fixtures/accessibility-test";

test.describe("Queue accessibility", () => {
  test.describe.configure({ retries: 0 });

  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
  });

  test("queue panel is reachable and announces its state", async ({ page }) => {
    const queueButton = page.getByRole("button", { name: "Queue" });
    await queueButton.click();

    await expect(page.getByText("Up Next")).toBeVisible();
    await expect(page.getByTestId("queue-panel")).toBeVisible();
    await expect(queueButton).toHaveAttribute("aria-pressed", "true");
  });

  test("empty queue state is readable by a screen reader", async ({ page }) => {
    const queueButton = page.getByRole("button", { name: "Queue" });
    await queueButton.click();
    await expect(page.getByText("Queue is empty")).toBeVisible();
  });

  test("queue panel has no axe violations when open", async ({
    page,
    a11y,
  }) => {
    await page.getByRole("button", { name: "Queue" }).click();
    await expect(page.getByTestId("queue-panel")).toBeVisible();
    await a11y.axeForThemes();
  });

  test("queue with items exposes drag instructions for assistive tech", async ({
    page,
    tauriMock,
  }) => {
    await page
      .getByRole("button", { name: "See You Again" })
      .click({ button: "right" });
    await expect
      .poll(async () => tauriMock.getLastNativeMenu())
      .toMatchObject({
        items: expect.arrayContaining([
          expect.objectContaining({ label: "Add to Queue" }),
        ]),
      });
    await tauriMock.clickNativeMenuItem("Add to Queue");

    await page.getByRole("button", { name: "Queue" }).click();
    await expect(
      page.getByTestId("queue-panel").getByText("See You Again"),
    ).toBeVisible({ timeout: 5000 });

    const instructions = page.locator(
      "#DndDescribedBy-0, [id^='DndDescribedBy']",
    );
    await expect(instructions.first()).toBeAttached();
    await expect(instructions.first()).toContainText(/drag|keyboard|space/i);
  });
});
