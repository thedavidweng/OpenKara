// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { createSettingsHarness } from "@/test-utils/settings-controller";
import type { IntegrityReport } from "@/types/ipc";
import { SettingsControllerContext } from "./SettingsController.context";
import { SettingsDialogHost } from "./SettingsDialogHost";

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string) => key,
      i18n: { changeLanguage: vi.fn() },
    }),
  };
});

const reportWithOneIssue: IntegrityReport = {
  checked_local_songs: 1,
  skipped_remote_songs: 0,
  missing_primary_media: [
    { song_hash: "hash-a", asset_type: "primary_media", path: "media/a.mp3" },
  ],
  empty_primary_media: [],
  missing_optional_assets: [],
  empty_optional_assets: [],
  orphaned_managed_files: [],
};

describe("SettingsDialogHost interactions", () => {
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
    document.body.innerHTML = "";
  });

  test("the integrity cleanup dialog's confirm button removes the selection", async () => {
    const removeMissingLibraryEntries = vi.fn(async () => ({
      deleted_song_hashes: ["hash-a"],
      skipped_song_hashes: [],
    }));
    const harness = createSettingsHarness({
      overrides: {
        library: {
          checkLibraryIntegrity: async () => reportWithOneIssue,
          removeMissingLibraryEntries,
        },
      },
    });
    await harness.controller.library.checkIntegrity();
    await harness.controller.maintenance.openDialog(
      "integrity_cleanup_confirm",
    );

    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    act(() => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <SettingsDialogHost />
        </SettingsControllerContext>,
      );
    });

    const buttons = document.body.querySelectorAll("button");
    const confirmButton = buttons[buttons.length - 1] as HTMLButtonElement;

    await act(async () => {
      confirmButton.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(removeMissingLibraryEntries).toHaveBeenCalledWith(["hash-a"]);
  });
});
