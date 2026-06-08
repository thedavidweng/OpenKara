import { test as base } from "@playwright/test";
import { TAURI_MOCK_SCRIPT } from "./tauri-mock";

/**
 * Extended Playwright test that injects the Tauri IPC mock before each
 * test navigates to the app.  Every E2E spec should import `test` and
 * `expect` from this module instead of `@playwright/test`.
 */
export const test = base.extend<{}>({
  page: async ({ page }, use) => {
    // Inject the Tauri mock before the page navigates to the app.
    // addInitScript runs in every new document context, so it fires
    // before the React bundle starts executing.
    await page.addInitScript(TAURI_MOCK_SCRIPT);
    await use(page);
  },
});

export { expect } from "@playwright/test";
