import { expect, test } from "./fixtures/accessibility-test";

test.describe("Fullscreen player accessibility", () => {
  test("fullscreen audience view is marked as a presentation", async ({
    page,
  }) => {
    await page.goto("/?mode=fullscreen-player");
    await expect(page).toHaveURL("/?mode=fullscreen-player");

    await expect(
      page.locator('html[data-presentation-mode="audience"]'),
    ).toHaveCount(1);
    await expect(page.getByText("Select a song to start")).toBeVisible();
  });

  test.fixme("fullscreen controls are keyboard operable and do not hide focus", async ({
    page,
  }) => {
    test.fixme("TODO: implement fullscreen keyboard control checks");
    await page.goto("/?mode=fullscreen-player");
    await page.keyboard.press("Tab");
  });

  test.fixme("romanize and alignment controls have aria-pressed states", async ({
    page,
  }) => {
    test.fixme("TODO: implement toggle state checks");
    await page.goto("/?mode=fullscreen-player");
    await expect(page.getByTestId("fullscreen-romanize-button")).toBeVisible();
  });
});
