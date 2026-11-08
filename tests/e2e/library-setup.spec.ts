import { MISSING_LIBRARY_MOCK_DATA } from "@/mock/tauri-mock-data";
import { test, expect } from "./fixtures/base-test";
import { createTauriMockScript } from "./fixtures/tauri-mock";

/**
 * Library setup flow UI smoke tests.
 *
 * These tests verify the LibrarySetup wizard that appears when no library
 * is configured.  The Tauri mock defaults to a configured library, so
 * these tests override the mock to simulate first-run scenarios.
 */
test.describe("Library setup", () => {
  test("setup wizard appears when no library is registered", async ({
    page,
  }) => {
    // Override the Tauri mock to return an empty registry
    await page.addInitScript(`
      (() => {
        const origInvoke = window.__TAURI_INTERNALS__?.invoke;
        if (!origInvoke) return;
        window.__TAURI_INTERNALS__.invoke = (cmd, args) => {
          if (cmd === "get_library_registry") {
            return Promise.resolve({
              active_library_id: null,
              libraries: [],
            });
          }
          return origInvoke(cmd, args);
        };
      })();
    `);

    await page.goto("/");

    // The setup wizard should show a "welcome" heading and language selection
    // (language step comes first)
    await expect(
      page.getByText(/choose.*language|welcome/i).first(),
    ).toBeVisible({ timeout: 10000 });
  });

  test("language selection step renders options", async ({ page }) => {
    await page.addInitScript(`
      (() => {
        const origInvoke = window.__TAURI_INTERNALS__?.invoke;
        if (!origInvoke) return;
        window.__TAURI_INTERNALS__.invoke = (cmd, args) => {
          if (cmd === "get_library_registry") {
            return Promise.resolve({ active_library_id: null, libraries: [] });
          }
          return origInvoke(cmd, args);
        };
      })();
    `);

    await page.goto("/");

    // Language step: should show at least English and Chinese options
    await expect(page.getByText("English")).toBeVisible({ timeout: 10000 });
  });

  test("skipping to library step after language selection", async ({
    page,
  }) => {
    await page.addInitScript(`
      (() => {
        const origInvoke = window.__TAURI_INTERNALS__?.invoke;
        if (!origInvoke) return;
        window.__TAURI_INTERNALS__.invoke = (cmd, args) => {
          if (cmd === "get_library_registry") {
            return Promise.resolve({ active_library_id: null, libraries: [] });
          }
          return origInvoke(cmd, args);
        };
      })();
    `);

    await page.goto("/");
    await expect(page.getByText("English")).toBeVisible({ timeout: 10000 });

    // Select English
    await page.getByText("English").click();

    // Should advance to library step with "Create new" / "Open existing" options
    await expect(
      page.getByText(/create.*new.*library|create new/i).first(),
    ).toBeVisible({
      timeout: 5000,
    });
    await expect(page.getByText(/open.*existing/i).first()).toBeVisible();
  });

  test.describe("missing active library", () => {
    test.beforeEach(async ({ page }) => {
      await page.addInitScript(
        createTauriMockScript(MISSING_LIBRARY_MOCK_DATA),
      );
    });

    test("names the missing path and loads nothing from the library", async ({
      page,
      tauriMock,
    }) => {
      await page.goto("/");

      await expect(
        page.getByRole("heading", { name: "Your library could not be found" }),
      ).toBeVisible();
      await expect(page.getByText("/Users/demo/Music/OpenKara")).toBeVisible();
      await expect(
        page.getByRole("button", { name: /Try again/ }),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: /Locate library/ }),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: /Create new library/ }),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Backup Library" }),
      ).toBeVisible();

      const commands = (await tauriMock.getInvokeCalls()).map(
        (call) => call.cmd,
      );
      expect(commands).not.toContain("get_library");
      await expect(page.getByText(/Internal Error/)).toHaveCount(0);
    });

    test("keeps the recovery screen when the folder is still missing", async ({
      page,
    }) => {
      await page.goto("/");
      await page.getByRole("button", { name: /Try again/ }).click();

      await expect(page.getByRole("alert")).toContainText("still missing");
      await expect(
        page.getByRole("heading", { name: "Your library could not be found" }),
      ).toBeVisible();
    });

    test("opens another registered library", async ({ page }) => {
      await page.goto("/");
      await page.getByRole("button", { name: "Backup Library" }).click();

      await expect(
        page.getByRole("heading", { name: "Your library could not be found" }),
      ).toHaveCount(0);
      await expect(page.getByText("All Tracks")).toBeVisible();
    });
  });
});
