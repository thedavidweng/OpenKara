// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { createSettingsHarness } from "@/test-utils/settings-controller";
import type { IntegrityReport } from "@/types/ipc";
import { IntegrityReportModal } from "./IntegrityReportModal";
import { SettingsControllerContext } from "./SettingsController.context";

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

const reportWithSelectableIssues: IntegrityReport = {
  checked_local_songs: 2,
  skipped_remote_songs: 0,
  missing_primary_media: [
    {
      song_hash: "hash-toggle-test",
      asset_type: "primary_media",
      path: "media/missing.mp3",
    },
  ],
  empty_primary_media: [],
  missing_optional_assets: [],
  empty_optional_assets: [],
  orphaned_managed_files: [],
};

describe("IntegrityReportModal interactions", () => {
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

  test("the checkbox takes a song out of the cleanup selection", async () => {
    const harness = createSettingsHarness({
      overrides: {
        library: {
          checkLibraryIntegrity: async () => reportWithSelectableIssues,
        },
      },
    });
    await harness.controller.library.checkIntegrity();
    expect(harness.view().integrity.selection.has("hash-toggle-test")).toBe(
      true,
    );

    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    act(() => {
      root.render(
        <SettingsControllerContext value={harness.controller}>
          <IntegrityReportModal report={reportWithSelectableIssues} />
        </SettingsControllerContext>,
      );
    });

    const checkbox = container.querySelector(
      'input[type="checkbox"]',
    ) as HTMLInputElement;
    expect(checkbox).not.toBeNull();

    act(() => {
      checkbox.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(harness.view().integrity.selection.has("hash-toggle-test")).toBe(
      false,
    );
  });
});
