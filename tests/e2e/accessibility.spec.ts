import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures/base-test";

const WCAG_AA_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

async function expectNoAutomatableViolations(page: Page) {
  const results = await new AxeBuilder({ page })
    .withTags(WCAG_AA_TAGS)
    .analyze();

  expect(
    results.violations,
    results.violations
      .map(
        (violation) =>
          `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`,
      )
      .join("\n"),
  ).toEqual([]);
}

test.describe("Accessibility", () => {
  // A contrast result must describe a settled theme, not the short overlap
  // between individual components' color transitions during a theme switch.
  // Disable those visual-only transitions in this test and reject retries so
  // accessibility regressions cannot be hidden by a later attempt.
  test.describe.configure({ retries: 0 });

  for (const theme of ["dark", "light"] as const) {
    test(`${theme} app shell has no automated WCAG 2.2 A/AA violations`, async ({
      page,
    }) => {
      await page.goto("/");
      await expect(page.getByText("Earfquake")).toBeVisible();

      await page.addStyleTag({
        content: `
          *, *::before, *::after {
            animation: none !important;
            transition: none !important;
          }
        `,
      });

      await page.evaluate((selectedTheme) => {
        document.documentElement.dataset.theme = selectedTheme;
        document.documentElement.style.colorScheme = selectedTheme;
      }, theme);

      await page.waitForFunction(
        (selectedTheme) =>
          document.documentElement.dataset.theme === selectedTheme &&
          getComputedStyle(
            document.querySelector("[data-window-chrome-platform]")!,
          )
            .getPropertyValue("--color-text-dimmer")
            .trim() !== "",
        theme,
      );

      await expectNoAutomatableViolations(page);
    });
  }

  test("settings opens, traps focus, and closes through the keyboard", async ({
    page,
  }) => {
    await page.goto("/");
    const settingsButton = page.getByRole("button", { name: "Settings" });
    await settingsButton.focus();

    await page.keyboard.press("Enter");
    const settingsDialog = page.getByRole("dialog", { name: "Settings" });
    const closeButton = settingsDialog.getByRole("button", { name: "Close" });

    await expect(settingsDialog).toBeVisible();
    await expect(closeButton).toBeFocused();

    await page.keyboard.press("Escape");
    await expect(settingsDialog).toBeHidden();
    await expect(settingsButton).toBeFocused();
  });
});
