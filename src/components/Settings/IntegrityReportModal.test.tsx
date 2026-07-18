import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
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

const emptyReport: IntegrityReport = {
  checked_local_songs: 0,
  skipped_remote_songs: 0,
  missing_primary_media: [],
  empty_primary_media: [],
  missing_optional_assets: [],
  empty_optional_assets: [],
  orphaned_managed_files: [],
};

const sampleReport: IntegrityReport = {
  checked_local_songs: 5,
  skipped_remote_songs: 2,
  missing_primary_media: [
    {
      song_hash: "hash-aaaaaaaa",
      asset_type: "primary_media",
      path: "media/missing.mp3",
    },
  ],
  empty_primary_media: [
    {
      song_hash: "hash-bbbbbbbb",
      asset_type: "primary_media",
      path: "media/empty.mp3",
    },
  ],
  missing_optional_assets: [
    {
      song_hash: "hash-cccccccc",
      asset_type: "cdg",
      path: "media-g/missing.cdg",
    },
  ],
  empty_optional_assets: [
    {
      song_hash: "hash-dddddddd",
      asset_type: "stem_vocals",
      path: "stems/empty.wav",
    },
  ],
  orphaned_managed_files: ["stems/orphan1.wav", "stems/orphan2.wav"],
};

function renderWithContext(
  node: ReactElement,
  value: SettingsOverlayContextValue,
) {
  return renderToStaticMarkup(
    <SettingsOverlayContext value={value}>{node}</SettingsOverlayContext>,
  );
}

describe("IntegrityReportModal", () => {
  test("renders report title and summary counts", () => {
    const value = createSettingsOverlayTestContextValue();
    const markup = renderWithContext(
      <IntegrityReportModal report={sampleReport} />,
      value,
    );

    expect(markup).toContain("settings.integrity.reportTitle");
    expect(markup).toContain("settings.integrity.checkedLocal");
    expect(markup).toContain("count=5");
    expect(markup).toContain("settings.integrity.skippedRemote");
    expect(markup).toContain("count=2");
  });

  test("renders all five section headers with counts", () => {
    const value = createSettingsOverlayTestContextValue();
    const markup = renderWithContext(
      <IntegrityReportModal report={sampleReport} />,
      value,
    );

    expect(markup).toContain("settings.integrity.missingPrimary");
    expect(markup).toContain("settings.integrity.emptyPrimary");
    expect(markup).toContain("settings.integrity.missingOptional");
    expect(markup).toContain("settings.integrity.emptyOptional");
    expect(markup).toContain("settings.integrity.orphanedFiles");
  });

  test("renders no-issues message for empty sections", () => {
    const value = createSettingsOverlayTestContextValue();
    const markup = renderWithContext(
      <IntegrityReportModal report={emptyReport} />,
      value,
    );

    // All sections should show "no issues"
    const noIssuesCount = (markup.match(/settings\.integrity\.noIssues/g) ?? [])
      .length;
    expect(noIssuesCount).toBe(5);
    expect(markup).toContain("settings.integrity.allClean");
  });

  test("renders checkboxes for primary media issues (selectable)", () => {
    const value = createSettingsOverlayTestContextValue();
    const markup = renderWithContext(
      <IntegrityReportModal report={sampleReport} />,
      value,
    );

    // Checkboxes are rendered for selectable sections (missing + empty primary)
    const checkboxCount = (markup.match(/type="checkbox"/g) ?? []).length;
    expect(checkboxCount).toBe(2);
  });

  test("renders asset type labels for known asset types", () => {
    const value = createSettingsOverlayTestContextValue();
    const markup = renderWithContext(
      <IntegrityReportModal report={sampleReport} />,
      value,
    );

    expect(markup).toContain("settings.integrity.assetTypePrimaryMedia");
    expect(markup).toContain("settings.integrity.assetTypeCdg");
    expect(markup).toContain("settings.integrity.assetTypeStemVocals");
  });

  test("renders orphaned file paths", () => {
    const value = createSettingsOverlayTestContextValue();
    const markup = renderWithContext(
      <IntegrityReportModal report={sampleReport} />,
      value,
    );

    expect(markup).toContain("stems/orphan1.wav");
    expect(markup).toContain("stems/orphan2.wav");
  });

  test("renders remove-selected button", () => {
    const value = createSettingsOverlayTestContextValue({
      state: { integritySelection: new Set(["hash-aaaaaaaa"]) },
    });
    const markup = renderWithContext(
      <IntegrityReportModal report={sampleReport} />,
      value,
    );

    expect(markup).toContain("settings.integrity.removeSelected");
  });

  test("renders skipped notice when integritySkippedCount is set", () => {
    const value = createSettingsOverlayTestContextValue({
      state: { integritySkippedCount: 3 },
    });
    const markup = renderWithContext(
      <IntegrityReportModal report={sampleReport} />,
      value,
    );

    expect(markup).toContain("settings.integrity.skippedNotice");
    expect(markup).toContain("count=3");
  });

  test("does not render skipped notice when integritySkippedCount is null", () => {
    const value = createSettingsOverlayTestContextValue();
    const markup = renderWithContext(
      <IntegrityReportModal report={sampleReport} />,
      value,
    );

    expect(markup).not.toContain("settings.integrity.skippedNotice");
  });

  test("renders close button", () => {
    const value = createSettingsOverlayTestContextValue();
    const markup = renderWithContext(
      <IntegrityReportModal report={emptyReport} />,
      value,
    );

    expect(markup).toContain("common.close");
  });

  test("renders song hash prefix in issue rows", () => {
    const value = createSettingsOverlayTestContextValue();
    const markup = renderWithContext(
      <IntegrityReportModal report={sampleReport} />,
      value,
    );

    expect(markup).toContain("hash-aa");
    expect(markup).toContain("hash-bb");
  });

  test("falls back to raw asset type label for unknown asset types", () => {
    const report: IntegrityReport = {
      ...emptyReport,
      missing_optional_assets: [
        {
          song_hash: "hash-unknown",
          asset_type: "custom_sidecar",
          path: "extra/file.bin",
        },
      ],
    };
    const value = createSettingsOverlayTestContextValue();
    const markup = renderWithContext(
      <IntegrityReportModal report={report} />,
      value,
    );
    expect(markup).toContain("custom_sidecar");
    expect(markup).toContain("extra/file.bin");
  });

  test("renders empty-path placeholder for issues without a path", () => {
    const report: IntegrityReport = {
      ...emptyReport,
      missing_optional_assets: [
        {
          song_hash: "hash-nopath",
          asset_type: "cdg",
          path: "",
        },
      ],
    };
    const value = createSettingsOverlayTestContextValue();
    const markup = renderWithContext(
      <IntegrityReportModal report={report} />,
      value,
    );
    expect(markup).toContain("settings.integrity.emptyPath");
  });

  test("shows deleting label while integrity cleanup is in progress", () => {
    const value = createSettingsOverlayTestContextValue({
      state: { integritySelection: new Set(["hash-aaaaaaaa"]) },
      meta: { integrityCleanupInProgress: true },
    });
    const markup = renderWithContext(
      <IntegrityReportModal report={sampleReport} />,
      value,
    );

    expect(markup).toContain("common.deleting");
    expect(markup).toContain("disabled");
  });

  test("does not show skipped notice when integritySkippedCount is zero", () => {
    const value = createSettingsOverlayTestContextValue({
      state: { integritySkippedCount: 0 },
    });
    const markup = renderWithContext(
      <IntegrityReportModal report={sampleReport} />,
      value,
    );

    expect(markup).not.toContain("settings.integrity.skippedNotice");
  });
});
