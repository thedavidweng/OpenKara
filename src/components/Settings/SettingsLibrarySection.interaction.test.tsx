// @vitest-environment jsdom

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { SettingsLibrarySection } from "./SettingsLibrarySection";
import {
  SettingsOverlayContext,
  createSettingsOverlayTestContextValue,
  type SettingsOverlayContextValue,
} from "./SettingsOverlay.context";

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, params?: Record<string, unknown>) => {
        if (typeof params === "object" && params !== null) {
          const entries = Object.entries(params)
            .map(([k, v]) => `${k}=${String(v)}`)
            .join(",");
          return `${key}{${entries}}`;
        }
        return key;
      },
      i18n: { changeLanguage: vi.fn() },
    }),
  };
});

function renderWithContext(
  node: ReactElement,
  value: SettingsOverlayContextValue,
) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => {
    root.render(
      <SettingsOverlayContext value={value}>{node}</SettingsOverlayContext>,
    );
  });
  return { container, root };
}

describe("SettingsLibrarySection interactions", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  test("clicking integrity check button calls checkLibraryIntegrity", () => {
    const checkLibraryIntegrity = vi.fn().mockResolvedValue(undefined);
    const value = createSettingsOverlayTestContextValue(
      {
        state: {
          libraries: [
            {
              id: "local:/karaoke",
              kind: "local",
              display_name: "Main Library",
              root_path: "/karaoke",
            },
          ],
          activeLibraryId: "local:/karaoke",
        },
        meta: { isInitializing: false },
      },
      { checkLibraryIntegrity },
    );

    const rendered = renderWithContext(<SettingsLibrarySection />, value);
    container = rendered.container;
    root = rendered.root;

    // The integrity check button has title "settings.integrity.checkButton".
    const integrityButton = container.querySelector(
      'button[title*="integrity.checkButton"]',
    ) as HTMLButtonElement;
    expect(integrityButton).not.toBeNull();
    expect(integrityButton.disabled).toBe(false);

    act(() => {
      integrityButton.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(checkLibraryIntegrity).toHaveBeenCalledOnce();
  });

  test("shows spinning refresh icon while integrity check is in progress", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        libraries: [
          {
            id: "local:/karaoke",
            kind: "local",
            display_name: "Main Library",
            root_path: "/karaoke",
          },
        ],
        activeLibraryId: "local:/karaoke",
      },
      meta: {
        isInitializing: false,
        integrityCheckInProgress: true,
      },
    });

    const rendered = renderWithContext(<SettingsLibrarySection />, value);
    container = rendered.container;
    root = rendered.root;

    const integrityButton = container.querySelector(
      'button[title*="integrity.checkButton"]',
    ) as HTMLButtonElement;
    expect(integrityButton).not.toBeNull();
    expect(integrityButton.disabled).toBe(true);
    expect(integrityButton.querySelector(".animate-spin")).not.toBeNull();
  });

  test("renders integrity report modal when report is present", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        libraries: [
          {
            id: "local:/karaoke",
            kind: "local",
            display_name: "Main Library",
            root_path: "/karaoke",
          },
        ],
        activeLibraryId: "local:/karaoke",
        integrityReport: {
          checked_local_songs: 0,
          skipped_remote_songs: 0,
          missing_primary_media: [],
          empty_primary_media: [],
          missing_optional_assets: [],
          empty_optional_assets: [],
          orphaned_managed_files: [],
        },
      },
      meta: { isInitializing: false },
    });

    const rendered = renderWithContext(<SettingsLibrarySection />, value);
    container = rendered.container;
    root = rendered.root;

    expect(container.textContent).toContain("settings.integrity.reportTitle");
  });

  test("disables integrity check while cleanup is in progress", () => {
    const value = createSettingsOverlayTestContextValue({
      state: {
        libraries: [
          {
            id: "local:/karaoke",
            kind: "local",
            display_name: "Main Library",
            root_path: "/karaoke",
          },
        ],
        activeLibraryId: "local:/karaoke",
      },
      meta: {
        isInitializing: false,
        integrityCleanupInProgress: true,
      },
    });

    const rendered = renderWithContext(<SettingsLibrarySection />, value);
    container = rendered.container;
    root = rendered.root;

    const integrityButton = container.querySelector(
      'button[title*="integrity.checkButton"]',
    ) as HTMLButtonElement;
    expect(integrityButton).not.toBeNull();
    expect(integrityButton.disabled).toBe(true);
    expect(integrityButton.querySelector(".animate-spin")).toBeNull();
  });
});
