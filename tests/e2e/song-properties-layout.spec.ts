import { test, expect } from "./fixtures/base-test";

test.describe("Song Properties layout", () => {
  test("keeps re-separation controls inside a narrow dialog", async ({
    page,
    tauriMock,
  }) => {
    await page.setViewportSize({ width: 480, height: 568 });
    await page.goto("/");
    await expect(page.getByText("Earfquake")).toBeVisible();

    await tauriMock.setSeparationCompleted("earfquake");
    await page.getByRole("button", { name: "Earfquake" }).click({
      button: "right",
    });
    await tauriMock.clickNativeMenuItem("Properties");

    const dialog = page.getByRole("dialog", { name: "Song Properties" });
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText("Sample Rate");

    await dialog.getByRole("button", { name: "Re-separate" }).click();

    const dialogBox = await dialog.boundingBox();
    expect(dialogBox).not.toBeNull();

    for (const label of ["2-stem", "4-stem", "Confirm"]) {
      const buttonBox = await dialog
        .getByRole("button", { name: label, exact: true })
        .boundingBox();
      expect(buttonBox).not.toBeNull();
      expect(buttonBox!.x + buttonBox!.width).toBeLessThanOrEqual(
        dialogBox!.x + dialogBox!.width,
      );
    }
  });
});
