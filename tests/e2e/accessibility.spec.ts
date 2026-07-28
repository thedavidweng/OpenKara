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
  for (const theme of ["dark", "light"] as const) {
    test(`${theme} app shell has no automated WCAG 2.2 A/AA violations`, async ({
      page,
    }) => {
      await page.goto("/");
      await expect(page.getByText("Earfquake")).toBeVisible();

      await page.evaluate((selectedTheme) => {
        document.documentElement.dataset.theme = selectedTheme;
        document.documentElement.style.colorScheme = selectedTheme;
      }, theme);

      await expectNoAutomatableViolations(page);
    });
  }
});
