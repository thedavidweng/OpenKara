import { expect, test } from "./fixtures/accessibility-test";

test.describe("Queue accessibility", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL("/");
  });

  test("queue panel is reachable and announces its state", async ({ page }) => {
    const queueButton = page.getByRole("button", { name: "Queue" });
    await queueButton.click();

    await expect(page.getByText("Up Next")).toBeVisible();
    await expect(page.getByTestId("queue-panel")).toBeVisible();
    await expect(queueButton).toHaveAttribute("aria-pressed", "true");
  });

  test.fixme("queue drag and drop exposes keyboard instructions and announces changes", async ({
    page,
    a11y,
  }) => {
    test.fixme("TODO: implement dnd keyboard and announcement checks");
    await page.getByRole("button", { name: "Queue" }).click();
    await a11y.startLiveRegionMonitor();
  });

  test.fixme("empty queue state is readable by a screen reader", async ({
    page,
  }) => {
    test.fixme("TODO: implement empty queue status checks");
    await page.getByRole("button", { name: "Queue" }).click();
    await expect(page.getByText("Queue is empty")).toBeVisible();
  });
});
