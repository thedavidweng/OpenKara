import { expect, test } from "./fixtures/accessibility-test";

test("NetEase sign-in controls work from the keyboard", async ({
  page,
  tauriMock,
  a11y,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByTestId("online-source-netease").check();
  await page
    .getByRole("dialog", { name: "Preferences" })
    .getByRole("button", { name: "Close" })
    .click();
  await page.getByRole("button", { name: "NetEase Cloud Music" }).click();

  const connect = page.getByTestId("netease-connect");
  await expect(connect).toBeVisible();
  await expect(page.getByTestId("netease-check-signin")).toHaveCount(0);
  await expect(page.getByTestId("netease-signin").locator("input")).toHaveCount(
    0,
  );
  await a11y.axeCheck();

  await connect.focus();
  await page.keyboard.press("Enter");
  await expect
    .poll(async () =>
      (await tauriMock.getInvokeCalls()).some(
        (call) => call.cmd === "open_streaming_sign_in",
      ),
    )
    .toBe(true);
});
