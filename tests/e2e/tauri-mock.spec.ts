import { test, expect } from "./fixtures/base-test";

test.describe("Tauri IPC mock contract", () => {
  test("fails fast for unhandled invoke commands", async ({ page }) => {
    await page.goto("/");

    await expect(
      page.evaluate(() =>
        window.__TAURI_INTERNALS__.invoke("unknown_test_command", {}),
      ),
    ).rejects.toThrow("Unhandled Tauri invoke in E2E mock");
  });
});

declare global {
  interface Window {
    __TAURI_INTERNALS__: {
      invoke: (cmd: string, args?: unknown) => Promise<unknown>;
    };
  }
}
