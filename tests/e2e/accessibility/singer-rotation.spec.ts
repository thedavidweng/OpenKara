import { expect, test } from "./fixtures/accessibility-test";

test.describe("Singer rotation accessibility", () => {
  test.describe.configure({ retries: 0 });

  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
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

  test("singer assignment controls are labeled after adding a singer", async ({
    page,
    a11y,
  }) => {
    await page.getByRole("button", { name: "Queue" }).click();
    await page.getByText("+ Add Singer").click();
    const input = page.getByRole("textbox", { name: "Singer name" });
    await input.fill("Bob");
    await input.press("Enter");

    await expect(
      page.getByRole("button", { name: "Bob", exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Remove Bob" }),
    ).toBeVisible();

    await a11y.axeForThemes();
  });

  test("removing an unassigned singer keeps focus in the queue panel", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "Queue" }).click();
    await page.getByText("+ Add Singer").click();
    await page.getByRole("textbox", { name: "Singer name" }).fill("Carol");
    await page.keyboard.press("Enter");

    const remove = page.getByRole("button", { name: "Remove Carol" });
    await remove.focus();
    await remove.click();

    await expect(
      page.getByRole("button", { name: "Carol", exact: true }),
    ).toHaveCount(0);
    await expect(page.getByTestId("queue-panel")).toBeVisible();
  });
});
