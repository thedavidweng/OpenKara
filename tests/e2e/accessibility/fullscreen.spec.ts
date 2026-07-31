import { expect, test } from "./fixtures/accessibility-test";

test.describe("Fullscreen player accessibility", () => {
  test.describe.configure({ retries: 0 });

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

  test("fullscreen view has no axe violations in dark and light themes", async ({
    page,
    a11y,
  }) => {
    await page.goto("/?mode=fullscreen-player");
    await expect(page.getByText("Select a song to start")).toBeVisible();
    await a11y.axeForThemes();
  });

  test("fullscreen controls are keyboard operable after pointer wake", async ({
    page,
  }) => {
    await page.goto("/?mode=fullscreen-player");
    await page.mouse.move(400, 400);
    await page.mouse.move(420, 420);

    const romanize = page.getByTestId("fullscreen-romanize-button");
    await expect(romanize).toBeVisible({ timeout: 5000 });
    await expect(romanize).toHaveAttribute("aria-pressed", /true|false/);

    const play = page.getByRole("button", { name: /play|pause/i }).first();
    await play.focus();
    await expect(play).toBeFocused();
    await expect(play).toBeEnabled();
  });

  test("romanize and alignment controls expose aria-pressed states", async ({
    page,
  }) => {
    await page.goto("/?mode=fullscreen-player");
    await page.mouse.move(400, 400);

    const romanize = page.getByTestId("fullscreen-romanize-button");
    const alignment = page.getByTestId("fullscreen-alignment-button");
    await expect(romanize).toBeVisible({ timeout: 5000 });
    await expect(alignment).toBeVisible();
    await expect(romanize).toHaveAttribute("aria-pressed", /true|false/);
    await expect(alignment).toHaveAttribute("aria-pressed", /true|false/);
  });
});
