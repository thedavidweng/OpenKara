import { expect, test } from "./fixtures/accessibility-test";

test.describe("Singer rotation accessibility", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL("/");
  });

  test("singer rotation inputs and tags are keyboard usable", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "Queue" }).click();
    await page.getByText("+ Add Singer").click();

    const input = page.getByRole("textbox", { name: "Singer name" });
    await expect(input).toBeVisible();
    await input.fill("Alice");
    await input.press("Enter");

    await expect(
      page.getByRole("button", { name: "Alice", exact: true }),
    ).toBeVisible();
  });

  test.fixme("assigning a singer to a queue entry is announced", async ({
    page,
    a11y,
  }) => {
    test.fixme("TODO: implement singer assignment announcement checks");
    await page.getByRole("button", { name: "Queue" }).click();
    await a11y.startLiveRegionMonitor();
    await page.getByRole("textbox", { name: "Singer name" }).fill("Bob");
    await page.keyboard.press("Enter");
  });

  test.fixme("removing a singer warns and keeps focus inside the dialog", async ({
    page,
  }) => {
    test.fixme("TODO: implement remove-singer dialog focus checks");
    await page.getByRole("button", { name: "Queue" }).click();
    await page.getByText("+ Add Singer").click();
    await page.getByRole("textbox", { name: "Singer name" }).fill("Carol");
    await page.keyboard.press("Enter");
    await page.getByRole("button", { name: /Remove Carol/i }).click();
  });
});
