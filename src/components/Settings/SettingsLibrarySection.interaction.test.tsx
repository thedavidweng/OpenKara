// @vitest-environment jsdom

import { fireEvent } from "@testing-library/react";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { SettingsController } from "@/lib/settings-controller";
import {
  createInitializedSettingsHarness,
  type SettingsHarnessOptions,
} from "@/test-utils/settings-controller";
import type { IntegrityReport, RegisteredLibrary } from "@/types/ipc";
import { SettingsControllerContext } from "./SettingsController.context";
import { SettingsLibrarySection } from "./SettingsLibrarySection";

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

const localLibrary: RegisteredLibrary = {
  id: "local:/karaoke",
  kind: "local",
  display_name: "Main Library",
  root_path: "/karaoke",
};

const cleanReport: IntegrityReport = {
  checked_local_songs: 0,
  skipped_remote_songs: 0,
  missing_primary_media: [],
  empty_primary_media: [],
  missing_optional_assets: [],
  empty_optional_assets: [],
  orphaned_managed_files: [],
};

function activeLocalLibraryHarness(overrides?: SettingsHarnessOptions) {
  return createInitializedSettingsHarness({
    libraries: [localLibrary],
    activeLibraryId: localLibrary.id,
    ...overrides,
  });
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
    vi.restoreAllMocks();
  });

  function renderSection(controller: SettingsController) {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    act(() => {
      root.render(
        <SettingsControllerContext value={controller}>
          <SettingsLibrarySection />
        </SettingsControllerContext>,
      );
    });
    return container.querySelector(
      'button[title*="integrity.checkButton"]',
    ) as HTMLButtonElement;
  }

  test("clicking the integrity check button runs a check", async () => {
    const checkLibraryIntegrity = vi.fn(async () => cleanReport);
    const harness = await activeLocalLibraryHarness({
      overrides: { library: { checkLibraryIntegrity } },
    });

    const integrityButton = renderSection(harness.controller);
    expect(integrityButton).not.toBeNull();
    expect(integrityButton.disabled).toBe(false);

    await act(async () => {
      integrityButton.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(checkLibraryIntegrity).toHaveBeenCalledOnce();
  });

  test("shows a spinning refresh icon while the check is in progress", async () => {
    const harness = await activeLocalLibraryHarness({
      overrides: {
        library: { checkLibraryIntegrity: () => new Promise(() => {}) },
      },
    });
    void harness.controller.library.checkIntegrity();

    const integrityButton = renderSection(harness.controller);

    expect(integrityButton.disabled).toBe(true);
    expect(integrityButton.querySelector(".animate-spin")).not.toBeNull();
  });

  test("renders the integrity report modal when a report is present", async () => {
    const harness = await activeLocalLibraryHarness({
      overrides: {
        library: { checkLibraryIntegrity: async () => cleanReport },
      },
    });
    await harness.controller.library.checkIntegrity();

    renderSection(harness.controller);

    expect(container.textContent).toContain("settings.integrity.reportTitle");
  });

  async function openDeleteConfirmDialog(): Promise<HTMLInputElement> {
    const deleteButton = container.querySelector(
      'button[title="settings.library.deleteLibrary"]',
    ) as HTMLButtonElement;
    await act(async () => {
      deleteButton.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    return document.body.querySelector(
      'input[aria-label^="settings.library.typeToConfirmDelete"]',
    ) as HTMLInputElement;
  }

  function dialogConfirmButton(): HTMLButtonElement {
    const confirm = [...document.body.querySelectorAll("button")].find(
      (button) => button.textContent === "common.confirm",
    );
    if (!confirm) {
      throw new Error("delete confirmation button not found");
    }
    return confirm;
  }

  test("a mismatched delete confirmation name disables confirm and hints inline", async () => {
    const harness = await activeLocalLibraryHarness();
    renderSection(harness.controller);

    const input = await openDeleteConfirmDialog();
    await act(async () => {
      fireEvent.change(input, { target: { value: "Wrong Library" } });
    });

    expect(dialogConfirmButton().disabled).toBe(true);
    expect(input.getAttribute("aria-invalid")).toBe("true");
    expect(
      document.getElementById("input-dialog-mismatch-hint")?.textContent,
    ).toBe("settings.library.confirmNameMismatch{displayName=Main Library}");
  });

  test("the exact delete confirmation name enables confirm and deletes", async () => {
    const deleteLibrary = vi.fn(async () => ({
      active_library_id: null,
      libraries: [],
    }));
    const harness = await activeLocalLibraryHarness({
      overrides: { librarySetup: { deleteLibrary } },
    });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    renderSection(harness.controller);

    const input = await openDeleteConfirmDialog();
    await act(async () => {
      fireEvent.change(input, { target: { value: "Main Library" } });
    });

    expect(document.getElementById("input-dialog-mismatch-hint")).toBeNull();
    const confirmButton = dialogConfirmButton();
    expect(confirmButton.disabled).toBe(false);

    await act(async () => {
      confirmButton.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(deleteLibrary).toHaveBeenCalledWith(localLibrary.id);
  });

  test("disables the integrity check while a cleanup is in progress", async () => {
    const harness = await activeLocalLibraryHarness({
      overrides: {
        library: {
          checkLibraryIntegrity: async () => ({
            ...cleanReport,
            missing_primary_media: [
              {
                song_hash: "hash-a",
                asset_type: "primary_media",
                path: "media/a.mp3",
              },
            ],
          }),
          removeMissingLibraryEntries: () => new Promise(() => {}),
        },
      },
    });
    await harness.controller.library.checkIntegrity();
    harness.controller.library.dismissIntegrityReport();
    harness.controller.library.toggleIntegrityEntry("hash-a");
    void harness.controller.library.cleanUpIntegrity();

    const integrityButton = renderSection(harness.controller);

    expect(integrityButton.disabled).toBe(true);
    expect(integrityButton.querySelector(".animate-spin")).toBeNull();
  });
});
