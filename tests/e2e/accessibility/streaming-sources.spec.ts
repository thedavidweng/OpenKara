import { expect, test } from "./fixtures/accessibility-test";

test("source registries and the market work from the keyboard", async ({
  page,
  a11y,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Preferences" });
  await expect(dialog).toBeVisible();

  const registries = dialog.getByRole("region", { name: "Source Registries" });
  await expect(
    registries.getByRole("heading", { name: "Demo Registry" }),
  ).toBeVisible();

  const url = registries.getByRole("textbox", { name: "Registry URL" });
  await url.fill("https://extra.demo.test/registry.json");
  await registries.getByRole("button", { name: "Add registry" }).click();
  const addConfirm = page.getByRole("dialog", { name: "Add Source Registry" });
  await expect(addConfirm).toBeVisible();
  await expect(addConfirm).toContainText(
    "https://extra.demo.test/registry.json",
  );
  await expect(addConfirm).toContainText("maintained outside OpenKara");
  await addConfirm.getByRole("button", { name: "Add registry" }).press("Enter");
  await expect(
    registries.getByRole("heading", { name: "extra.demo.test" }),
  ).toBeVisible();

  const market = dialog.getByRole("region", { name: "Source Market" });
  const fresh = market.getByRole("article", { name: "Fresh Library" });
  await expect(fresh).toBeVisible();
  await expect(fresh).toContainText("https://fresh.demo.test");
  await a11y.axeCheck();

  await fresh.getByRole("button", { name: "Install", exact: true }).click();
  const install = page.getByRole("dialog", { name: "Install Fresh Library" });
  await expect(install).toBeVisible();
  await expect(install).toContainText("https://fresh.demo.test");
  await install.getByRole("button", { name: "Install package" }).press("Enter");
  await expect(fresh).toContainText("Installed");
  await expect(fresh).toContainText("Disabled");

  const web = market.getByRole("article", { name: "Web Library" });
  await web.getByRole("button", { name: "Update to 1.1.0" }).click();
  const consent = page.getByRole("dialog", { name: "Review new access" });
  await expect(consent).toBeVisible();
  await expect(consent).toContainText("https://api.demo.test");
  await consent
    .getByRole("button", { name: "Allow and update" })
    .press("Enter");
  await expect(
    web.getByRole("button", { name: "Update to 1.1.0" }),
  ).toHaveCount(0);
  await expect(web.getByText("1.1.0 · Ready")).toBeVisible();
  await a11y.axeCheck();

  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});

test("source authentication and capability tabs work from the keyboard", async ({
  page,
  a11y,
}) => {
  await page.goto("/");
  await page.evaluate(() =>
    window.__OPENKARA_E2E__.setCommandDelayMs("poll_streaming_sign_in", 10000),
  );
  await page.getByRole("button", { name: "Web Library", exact: true }).click();
  const browser = page.getByTestId("streaming-browser");
  await expect(
    browser.getByRole("heading", { name: "Web Library", exact: true }),
  ).toBeVisible();
  const liked = browser.getByRole("tab", { name: "Liked songs", exact: true });
  await liked.focus();
  await page.keyboard.press("ArrowRight");
  await expect(
    browser.getByRole("tab", { name: "Playlists", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Home");
  await expect(liked).toBeFocused();

  const webSignIn = browser
    .getByRole("button", { name: "Sign in", exact: true })
    .first();
  await webSignIn.focus();
  await page.keyboard.press("Enter");
  await expect(
    browser.getByText("Complete sign-in in the source window.").first(),
  ).toBeVisible();
  await a11y.axeCheck();

  await page
    .getByRole("button", { name: "Plugin Library", exact: true })
    .click();
  await expect(
    browser.getByRole("heading", { name: "Plugin Library", exact: true }),
  ).toBeVisible();
  const search = browser.getByRole("textbox", { name: "Search source songs" });
  await search.fill("Track");
  await search.press("Enter");
  await expect(
    browser.getByRole("tab", { name: "Search results", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  const playlists = browser.getByRole("tab", {
    name: "Playlists",
    exact: true,
  });
  await playlists.focus();
  await page.keyboard.press("ArrowRight");
  await expect(
    browser.getByRole("tab", { name: "Search results", exact: true }),
  ).toBeFocused();
  const pluginSignIn = browser
    .getByRole("button", { name: "Sign in", exact: true })
    .first();
  await pluginSignIn.focus();
  await page.keyboard.press("Enter");
  await expect(
    browser.getByText("Scan the UI-only demo code").first(),
  ).toBeVisible();
  await a11y.axeCheck();
  const cancel = browser
    .getByRole("button", { name: "Cancel", exact: true })
    .first();
  await cancel.focus();
  await page.keyboard.press("Enter");
  await expect(
    browser.getByRole("button", { name: "Sign in", exact: true }).first(),
  ).toBeVisible();

  await page.getByRole("button", { name: "Web Library", exact: true }).click();
  await expect(
    browser.getByRole("heading", { name: "Web Library", exact: true }),
  ).toBeVisible();
  await expect(liked).toBeVisible();
});

test("source switching keeps colliding remote tracks and cursor pages distinct", async ({
  page,
  a11y,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Web Library", exact: true }).click();
  const browser = page.getByTestId("streaming-browser");
  const search = browser.getByRole("textbox", { name: "Search source songs" });
  await search.fill("Shared Track");
  await search.press("Enter");
  await expect(
    browser.getByTestId("streaming-track-row").first(),
  ).toContainText("Web Library: Shared Track");

  await page
    .getByRole("button", { name: "Plugin Library", exact: true })
    .click();
  await expect(search).toHaveValue("");
  await search.fill("Track");
  await search.press("Enter");
  await expect(
    browser.getByTestId("streaming-track-row").first(),
  ).toContainText("Plugin Library: Shared Track");
  const more = browser.getByRole("button", { name: "Load more", exact: true });
  await more.focus();
  await page.keyboard.press("Enter");
  await expect(
    browser.getByText("Plugin Library: Final Track", { exact: true }),
  ).toBeVisible();
  await a11y.axeCheck();

  await page.getByRole("button", { name: "Web Library", exact: true }).click();
  await expect(search).toHaveValue("Shared Track");
  await expect(
    browser.getByTestId("streaming-track-row").first(),
  ).toContainText("Web Library: Shared Track");
});
