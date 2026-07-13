import { test as base, expect as baseExpect } from "@playwright/test";
import { TAURI_MOCK_SCRIPT } from "./tauri-mock";

export interface TauriInvokeCall {
  args: unknown;
  cmd: string;
}

export interface NativeMenuSnapshot {
  items: Array<{
    children?: Array<{ label: string }>;
    label: string;
  }>;
  popupPosition: { x: number; y: number } | null;
}

/**
 * Patch accepted by `setPlaybackSnapshot`. Nested `stem_volumes` are merged
 * per-stem rather than replaced wholesale.
 */
export interface PlaybackSnapshotPatch {
  song_id?: string | null;
  state?: string;
  is_playing?: boolean;
  position_ms?: number;
  duration_ms?: number;
  buffered_ms?: number;
  volume?: number;
  has_stems?: boolean;
  stem_mode?: string | null;
  stem_volumes?: Partial<Record<string, number>>;
  transport_generation?: number;
}

export interface TauriMockHelpers {
  getInvokeCalls: () => Promise<TauriInvokeCall[]>;
  getLastNativeMenu: () => Promise<NativeMenuSnapshot | null>;
  clickNativeMenuItem: (label: string) => Promise<void>;
  clickNativeSubmenuItem: (parentLabel: string, label: string) => Promise<void>;
  setMockSongs: (songs: unknown[]) => Promise<void>;
  setMockLyrics: (lyrics: unknown) => Promise<void>;
  setPlaybackSnapshot: (
    patch: PlaybackSnapshotPatch,
  ) => Promise<Record<string, unknown>>;
  setSeparationCompleted: (songHash: string) => Promise<void>;
  getPlaybackSnapshot: () => Promise<Record<string, unknown>>;
}

/**
 * Extended Playwright test that injects the Tauri IPC mock before each
 * test navigates to the app.  Every UI smoke spec should import `test` and
 * `expect` from this module instead of `@playwright/test`.
 */
export const test = base.extend<{ tauriMock: TauriMockHelpers }>({
  page: async ({ page }, use) => {
    // Inject the Tauri mock before the page navigates to the app.
    // addInitScript runs in every new document context, so it fires
    // before the React bundle starts executing.
    await page.addInitScript(TAURI_MOCK_SCRIPT);
    await use(page);
  },
  tauriMock: async ({ page }, use) => {
    await use({
      getInvokeCalls: () =>
        page.evaluate(() => window.__OPENKARA_E2E__.getInvokeCalls()),
      getLastNativeMenu: () =>
        page.evaluate(() => window.__OPENKARA_E2E__.getLastNativeMenu()),
      clickNativeMenuItem: (label) =>
        page.evaluate(
          (itemLabel) => window.__OPENKARA_E2E__.clickNativeMenuItem(itemLabel),
          label,
        ),
      clickNativeSubmenuItem: (parentLabel, label) =>
        page.evaluate(
          ({ parent, item }) =>
            window.__OPENKARA_E2E__.clickNativeSubmenuItem(parent, item),
          { parent: parentLabel, item: label },
        ),
      setMockSongs: (songs) =>
        page.evaluate((s) => window.__OPENKARA_E2E__.setMockSongs(s), songs),
      setMockLyrics: (lyrics) =>
        page.evaluate((l) => window.__OPENKARA_E2E__.setMockLyrics(l), lyrics),
      setPlaybackSnapshot: (patch) =>
        page.evaluate(
          (p) => window.__OPENKARA_E2E__.setPlaybackSnapshot(p),
          patch,
        ),
      setSeparationCompleted: (songHash) =>
        page.evaluate(
          (hash) => window.__OPENKARA_E2E__.setSeparationCompleted(hash),
          songHash,
        ),
      getPlaybackSnapshot: () =>
        page.evaluate(() => window.__OPENKARA_E2E__.getPlaybackSnapshot()),
    });
  },
});

export const expect = baseExpect;

export function objectRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null
    ? (value as Record<string, unknown>)
    : null;
}

declare global {
  interface Window {
    __OPENKARA_E2E__: {
      clickNativeMenuItem: (label: string) => Promise<void>;
      clickNativeSubmenuItem: (
        parentLabel: string,
        label: string,
      ) => Promise<void>;
      emitEvent: (eventName: string, payload: unknown) => void;
      setCommandDelayMs: (cmd: string, delayMs: number) => void;
      setMockSongs: (songs: unknown[]) => void;
      setMockLyrics: (lyrics: unknown) => void;
      getInvokeCalls: () => TauriInvokeCall[];
      getLastNativeMenu: () => NativeMenuSnapshot | null;
      setPlaybackSnapshot: (patch: unknown) => Record<string, unknown>;
      setSeparationCompleted: (songHash: string) => void;
      getPlaybackSnapshot: () => Record<string, unknown>;
    };
  }
}
