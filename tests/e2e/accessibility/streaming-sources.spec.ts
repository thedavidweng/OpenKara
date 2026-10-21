import { expect, test } from "./fixtures/accessibility-test";

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
