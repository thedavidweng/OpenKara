import { defineConfig, devices } from "@playwright/test";
import baseConfig from "./playwright.config";

/**
 * WebKit variant of the UI smoke config.
 *
 * Tauri on macOS renders in WKWebView, so scroll/gesture-sensitive specs
 * (lyrics auto-follow) must be verified against WebKit — Chromium passing is
 * not sufficient evidence for production behavior.
 */
export default defineConfig({
  ...baseConfig,
  projects: [
    {
      name: "webkit",
      use: {
        ...devices["Desktop Safari"],
        viewport: { width: 1280, height: 800 },
      },
    },
  ],
});
