import { expect, test } from "./fixtures/accessibility-test";

test.describe("Remote libraries accessibility", () => {
  test.describe.configure({ retries: 0 });

  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();
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

  test("remote repository form fields have explicit labels", async ({
    page,
    a11y,
  }) => {
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("button", { name: "Add Remote Repository" }).click();

    const dialog = page.getByRole("dialog", {
      name: /add remote repository/i,
    });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByLabel("Display name")).toBeVisible();
    await a11y.axeForThemes();
  });

  test("webdav inputs expose autocomplete attributes and password masking", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("button", { name: "Add Remote Repository" }).click();
    await page.getByRole("button", { name: "WebDAV" }).click();

    await expect(page.getByLabel("Server URL")).toBeVisible();
    const password = page.getByLabel("Password");
    await expect(password).toBeVisible();
    await expect(password).toHaveAttribute("type", "password");
    await expect(password).toHaveAttribute("autocomplete", "current-password");

    const username = page.getByLabel(/username|user name/i);
    await expect(username).toBeVisible();
    await expect(username).toHaveAttribute("autocomplete", "username");
  });
});
