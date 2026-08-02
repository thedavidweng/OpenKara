import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright UI smoke configuration for OpenKara.
 *
 * These tests run against the Vite dev server (port 1420) with a mocked
 * Tauri IPC layer injected via fixtures (see tests/e2e/fixtures/base-test.ts).
 * They exercise the React UI in real Chromium and WebKit browsers without
 * requiring the Rust backend or any platform-specific Tauri dependencies.
 * WebKit is included because Tauri on macOS renders WKWebView; geometry and
 * pressed-state contracts must hold in both engines.
 *
 * For full desktop E2E (native window chrome, filesystem, audio pipeline)
 * use tauri-driver — see tests/e2e/SMOKE.md for manual smoke-test steps.
 */
export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  // Generate the list reporter (for live CI logs), the HTML report (for the
  // playwright-report/ artifact upload), and a machine-readable JSON report.
  // The CI workflow uploads playwright-report/ with if-no-files-found: error,
  // so the config and workflow must agree on HTML report generation. The JSON
  // report feeds the "Report flaky tests" step, which counts status=flaky
  // results so retries:1 can no longer silently absorb flakes.
  reporter: process.env.CI
    ? [
        ["list"],
        ["html", { open: "never" }],
        ["json", { outputFile: "playwright-report/results.json" }],
      ]
    : "list",
  timeout: 30_000,

  use: {
    baseURL: "http://localhost:1420",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },

  projects: [
    {
      name: "chromium-accessibility",
      testMatch: "accessibility/**/*.spec.ts",
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1280, height: 800 },
      },
    },
    {
      name: "webkit-accessibility",
      testMatch: "accessibility/**/*.spec.ts",
      use: {
        ...devices["Desktop Safari"],
        viewport: { width: 1280, height: 800 },
      },
    },
    {
      name: "chromium-smoke",
      testMatch: "smoke.spec.ts",
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1280, height: 800 },
      },
    },
    {
      name: "webkit-smoke",
      testMatch: "smoke.spec.ts",
      use: {
        ...devices["Desktop Safari"],
        viewport: { width: 1280, height: 800 },
      },
    },
    {
      name: "chromium-ui",
      testMatch: "*.spec.ts",
      testIgnore: ["accessibility/**/*.spec.ts", "smoke.spec.ts"],
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1280, height: 800 },
      },
    },
    {
      name: "webkit-ui",
      testMatch: "*.spec.ts",
      testIgnore: ["accessibility/**/*.spec.ts", "smoke.spec.ts"],
      use: {
        ...devices["Desktop Safari"],
        viewport: { width: 1280, height: 800 },
      },
    },
  ],

  /* Start the Vite dev server before running tests */
  webServer: {
    command: "pnpm dev",
    url: "http://localhost:1420",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
