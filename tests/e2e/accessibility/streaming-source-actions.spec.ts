import { expect, test } from "./fixtures/accessibility-test";
import type { Locator, Page } from "@playwright/test";

const openSources = async (page: Page) => {
  await page.getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Preferences" });
  await dialog.getByRole("tab", { name: "Online Sources" }).click();
  return dialog;
};

const chooseFromMore = async (page: Page, card: Locator, item: string) => {
  const more = card.getByRole("button", { name: /^More actions for / });
  await more.press("Enter");
  await page
    .getByRole("menu")
    .getByRole("menuitem", { name: item })
    .press("Enter");
  await expect(page.getByRole("menu")).toHaveCount(0);
};

test("every installed source action works from the keyboard", async ({
  page,
  a11y,
}) => {
  await page.goto("/");
  const sidebarSource = page.getByRole("button", {
    name: "Plugin Library",
    exact: true,
  });
  let dialog = await openSources(page);
  const card = dialog.getByRole("article", { name: "Plugin Library" });
  await expect(card).toContainText("Ready · 1.0.0 · Update available");

  await card.getByRole("button", { name: "Update to 1.1.0" }).press("Enter");
  await expect(card).toContainText("Ready · 1.1.0");
  await expect(
    card.getByRole("button", { name: "Update to 1.1.0" }),
  ).toHaveCount(0);

  await chooseFromMore(page, card, "Pin installed version");
  await expect(card).toContainText("Pinned to 1.1.0");
  await chooseFromMore(page, card, "Remove pin");
  await expect(card).not.toContainText("Pinned to");

  await chooseFromMore(page, card, "Disable");
  await expect(card).toContainText("Disabled");
  await expect(sidebarSource).toHaveCount(0);
  await expect(card.getByRole("button", { name: "Sign in" })).toHaveCount(0);
  await card.getByRole("button", { name: "Enable" }).press("Enter");
  await expect(card).toContainText("Ready · 1.1.0");
  await expect(sidebarSource).toBeVisible();
  await a11y.axeCheck();

  await card
    .getByRole("button", { name: "Sign in", exact: true })
    .press("Enter");
  await expect(dialog).toBeHidden();
  const browser = page.getByTestId("streaming-browser");
  await expect(
    browser.getByRole("heading", { name: "Plugin Library", exact: true }),
  ).toBeVisible();
  await expect(
    browser.getByRole("button", { name: /Account menu/ }),
  ).toBeVisible();

  dialog = await openSources(page);
  await expect(card).toContainText("Signed in");
  await card.getByRole("button", { name: "Open", exact: true }).press("Enter");
  await expect(dialog).toBeHidden();
  await expect(
    browser.getByRole("heading", { name: "Plugin Library", exact: true }),
  ).toBeVisible();

  dialog = await openSources(page);
  await chooseFromMore(page, card, "Sign out");
  await expect(card).not.toContainText("Signed in");
  await expect(
    card.getByRole("button", { name: "Sign in", exact: true }),
  ).toBeVisible();

  await chooseFromMore(page, card, "Uninstall");
  const confirm = page.getByRole("dialog", {
    name: /Plugin Library · Uninstall/,
  });
  await expect(confirm).toContainText("Imported songs and playlists stay");
  const uninstall = confirm.getByRole("button", { name: "Uninstall source" });
  await expect(uninstall).toBeDisabled();
  await confirm.getByRole("radio", { name: "Keep saved credentials" }).check();
  await a11y.axeCheck();
  await uninstall.press("Enter");
  await expect(confirm).toBeHidden();
  await expect(card).toHaveCount(0);
  await expect(sidebarSource).toHaveCount(0);
  await expect(
    dialog.getByRole("tab", { name: "Installed (1)" }),
  ).toBeVisible();

  await dialog.getByRole("tab", { name: "Discover" }).click();
  const listing = dialog.getByRole("article", { name: "Plugin Library" });
  await listing.getByRole("button", { name: "Install", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Install Plugin Library" })
    .getByRole("button", { name: "Install package" })
    .press("Enter");
  await dialog.getByRole("tab", { name: /^Installed/ }).click();
  await expect(
    dialog.getByRole("article", { name: "Plugin Library" }),
  ).toContainText("1.1.0");
});

test("Preferences tabs report the axis they are laid out on, and Tab leaves a source menu in order", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/");
  const dialog = await openSources(page);
  const pages = dialog.getByRole("tablist", { name: "Preferences sections" });
  await expect(pages).toHaveAttribute("aria-orientation", "vertical");
  await page.setViewportSize({ width: 680, height: 800 });
  await expect(pages).toHaveAttribute("aria-orientation", "horizontal");

  const card = dialog.getByRole("article", { name: "Plugin Library" });
  const more = card.getByRole("button", { name: /^More actions for / });
  await more.press("Enter");
  await expect(page.getByRole("menu")).toBeVisible();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("menu")).toHaveCount(0);
  await expect(page.locator("#settings-close")).not.toBeFocused();
  const follows = await more.evaluate((trigger) => {
    const active = document.activeElement;
    return (
      active !== null &&
      active !== trigger &&
      Boolean(
        trigger.compareDocumentPosition(active) &
        Node.DOCUMENT_POSITION_FOLLOWING,
      )
    );
  });
  expect(follows).toBe(true);
});
