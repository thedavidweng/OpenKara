// @vitest-environment jsdom

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { IntegrityReportModal } from "./IntegrityReportModal";
import {
  SettingsOverlayContext,
  createSettingsOverlayTestContextValue,
  type SettingsOverlayContextValue,
} from "./SettingsOverlay.context";
import type { IntegrityReport } from "@/types/ipc";

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

  test("checkbox toggle calls toggleIntegritySelection with song hash", () => {
    const toggleIntegritySelection = vi.fn();
    const value = createSettingsOverlayTestContextValue(
      {},
      { toggleIntegritySelection },
    );

    const rendered = renderWithContext(
      <IntegrityReportModal report={reportWithSelectableIssues} />,
      value,
    );
    container = rendered.container;
    root = rendered.root;

    const checkbox = container.querySelector(
      'input[type="checkbox"]',
    ) as HTMLInputElement;
    expect(checkbox).not.toBeNull();

    act(() => {
      checkbox.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(toggleIntegritySelection).toHaveBeenCalledWith("hash-toggle-test");
  });
});
