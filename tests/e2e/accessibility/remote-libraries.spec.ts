import { expect, test } from "./fixtures/accessibility-test";

test.describe("Remote libraries accessibility", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL("/");
  });

  test("remote repository wizard opens as a modal with a labeled dialog", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("button", { name: "Add Remote Repository" }).click();

    const dialog = page.getByRole("dialog", {
      name: /add remote repository/i,
    });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Close" })).toBeVisible();
    await expect(dialog.getByLabel("Display name")).toBeVisible();
  });

  test.fixme("remote repository form fields have explicit labels and errors are linked", async ({
    page,
    a11y,
  }) => {
    test.fixme("TODO: implement form label and error checks");
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("button", { name: "Add Remote Repository" }).click();
    await a11y.axeCheck();
  });

  test.fixme("webdav inputs expose autocomplete attributes and password masking", async ({
    page,
  }) => {
    test.fixme("TODO: implement webdav input accessibility checks");
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("button", { name: "Add Remote Repository" }).click();
    await page.getByRole("button", { name: "WebDAV" }).click();
    await expect(page.getByLabel("Server URL")).toBeVisible();
  });
});
