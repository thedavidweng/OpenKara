/**
 * Mock for Tauri IPC used by Playwright E2E tests.
 *
 * In a real Tauri build the Rust backend owns the database, audio pipeline,
 * and filesystem.  During browser-based UI smoke runs (against the Vite dev
 * server) none of that exists, so we stub `window.__TAURI_INTERNALS__` --
 * the single entry-point that every `invoke()` call from
 * `@tauri-apps/api/core` funnels through.
 *
 * The mock is injected via `page.addInitScript()` *before* the app bundle
 * executes.  The command handler logic lives in the shared module
 * `src/mock/tauri-mock-impl.ts` (also used by the website preview) so the
 * two surfaces cannot drift apart.  We serialize the `createTauriMock`
 * function via `Function.prototype.toString()` and inline the mock data as
 * JSON, producing a self-contained script that runs without imports.
 */
import { E2E_MOCK_DATA, MOCK_SIDEBAR_WIDTH } from "@/mock/tauri-mock-data";
import { createTauriMock } from "@/mock/tauri-mock-impl";

export { MOCK_SIDEBAR_WIDTH };

// Serialize the function source and data for page.addInitScript().
// createTauriMock is self-contained (no external references), so
// toString() produces a valid standalone function expression.
const fnSource = createTauriMock.toString();
const dataJson = JSON.stringify(E2E_MOCK_DATA);

export const TAURI_MOCK_SCRIPT = `
(() => {
  const result = (${fnSource})(${dataJson});
  window.__TAURI_INTERNALS__ = result.internals;
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = result.eventPluginInternals;
  window.__OPENKARA_E2E__ = result.helpers;

  // If a test set window.__OPENKARA_LARGE_LIBRARY_COUNT__ via addInitScript
  // before this mock runs, eagerly generate the synthetic catalog so the
  // initial get_library returns it without a reload.
  if (window.__OPENKARA_LARGE_LIBRARY_COUNT__ > 0) {
    result.helpers.setLargeLibrary(window.__OPENKARA_LARGE_LIBRARY_COUNT__);
  }
})();
`;
