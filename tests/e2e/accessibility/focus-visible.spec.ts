import { expect, test } from "./fixtures/accessibility-test";

test.describe("Focus-visible screenshot baselines", () => {
  test("page loads for focus-visible baseline capture", async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL("/");
  });

  test.fixme("primary button focus indicator is visible and stable", async () => {
    test.fixme(
      "TODO: capture baseline screenshot for primary button focus-visible state",
    );
  });

  test.fixme("secondary button focus indicator is visible and stable", async () => {
    test.fixme(
      "TODO: capture baseline screenshot for secondary button focus-visible state",
    );
  });

  test.fixme("icon button focus indicator is visible and stable", async () => {
    test.fixme(
      "TODO: capture baseline screenshot for icon button focus-visible state",
    );
  });

  test.fixme("sidebar item focus indicator is visible and stable", async () => {
    test.fixme(
      "TODO: capture baseline screenshot for sidebar item focus-visible state",
    );
  });

  test.fixme("list row focus indicator is visible and stable", async () => {
    test.fixme(
      "TODO: capture baseline screenshot for list row focus-visible state",
    );
  });

  test.fixme("slider focus indicator is visible and stable", async () => {
    test.fixme(
      "TODO: capture baseline screenshot for slider focus-visible state",
    );
  });

  test.fixme("checkbox focus indicator is visible and stable", async () => {
    test.fixme(
      "TODO: capture baseline screenshot for checkbox focus-visible state",
    );
  });

  test.fixme("segmented control focus indicator is visible and stable", async () => {
    test.fixme(
      "TODO: capture baseline screenshot for segmented control focus-visible state",
    );
  });

  test.fixme("text field focus indicator is visible and stable", async () => {
    test.fixme(
      "TODO: capture baseline screenshot for text field focus-visible state",
    );
  });

  test.fixme("dialog close button focus indicator is visible and stable", async () => {
    test.fixme(
      "TODO: capture baseline screenshot for dialog close button focus-visible state",
    );
  });

  test.fixme("destructive action focus indicator is visible and stable", async () => {
    test.fixme(
      "TODO: capture baseline screenshot for destructive action focus-visible state",
    );
  });
});
