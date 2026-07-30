import { expect, test } from "./fixtures/accessibility-test";

test.describe("Library accessibility", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL("/");
  });

  test("library controls have accessible names and the song list is reachable", async ({
    page,
  }) => {
    await expect(page.getByRole("textbox", { name: "Search" })).toBeVisible();
    await expect(page.getByRole("combobox", { name: "Sort by" })).toBeVisible();
    await expect(page.getByTestId("song-list")).toBeVisible();
    await expect(page.getByRole("button", { name: "Earfquake" })).toBeVisible();
  });

  test.fixme("virtualized song list exposes all rows to keyboard and screen readers", async ({
    page,
  }) => {
    test.fixme("TODO: implement virtual list accessibility checks");
    await page.getByRole("button", { name: "Earfquake" }).focus();
  });

  test.fixme("alphabet rail has correct labels and does not break focus order", async ({
    page,
  }) => {
    test.fixme("TODO: implement alphabet rail focus and label checks");
    await expect(page.getByRole("button", { name: /Jump to/i })).toBeVisible();
  });

  test.fixme("library has no axe violations after searching and filtering", async ({
    page,
    a11y,
  }) => {
    test.fixme("TODO: implement axe scan after search interaction");
    await page.getByRole("textbox", { name: "Search" }).fill("Earf");
    await a11y.axeCheck();
  });
});
