// @vitest-environment jsdom

import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import type { SettingsController } from "@/lib/settings-controller";
import { createSettingsHarness } from "@/test-utils/settings-controller";
import type { IntegrityReport } from "@/types/ipc";
import { IntegrityReportModal } from "./IntegrityReportModal";
import { SettingsControllerContext } from "./SettingsController.context";

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

function markupOf(node: ReactElement, controller: SettingsController) {
  return renderToStaticMarkup(
    <SettingsControllerContext value={controller}>
      {node}
    </SettingsControllerContext>,
  );
}

async function checkedHarness(report: IntegrityReport) {
  const harness = createSettingsHarness({
    overrides: { library: { checkLibraryIntegrity: async () => report } },
  });
  await harness.controller.library.checkIntegrity();
  return harness;
}

describe("IntegrityReportModal", () => {
  test("renders report title and summary counts", async () => {
    const harness = await checkedHarness(sampleReport);
    const markup = markupOf(
      <IntegrityReportModal report={sampleReport} />,
      harness.controller,
    );

    expect(markup).toContain("settings.integrity.reportTitle");
    expect(markup).toContain("settings.integrity.checkedLocal");
    expect(markup).toContain("count=5");
    expect(markup).toContain("settings.integrity.skippedRemote");
    expect(markup).toContain("count=2");
    expect(markup).toContain('role="dialog"');
    expect(markup).toContain('aria-modal="true"');
    expect(markup).toContain('aria-labelledby="integrity-report-modal-title"');
  });

  test("renders all five section headers with counts", async () => {
    const harness = await checkedHarness(sampleReport);
    const markup = markupOf(
      <IntegrityReportModal report={sampleReport} />,
      harness.controller,
    );

    expect(markup).toContain("settings.integrity.missingPrimary");
    expect(markup).toContain("settings.integrity.emptyPrimary");
    expect(markup).toContain("settings.integrity.missingOptional");
    expect(markup).toContain("settings.integrity.emptyOptional");
    expect(markup).toContain("settings.integrity.orphanedFiles");
  });

  test("renders no-issues message for empty sections", async () => {
    const harness = await checkedHarness(emptyReport);
    const markup = markupOf(
      <IntegrityReportModal report={emptyReport} />,
      harness.controller,
    );

    expect((markup.match(/settings\.integrity\.noIssues/g) ?? []).length).toBe(
      5,
    );
    expect(markup).toContain("settings.integrity.allClean");
  });

  test("renders checkboxes for primary media issues (selectable)", async () => {
    const harness = await checkedHarness(sampleReport);
    const markup = markupOf(
      <IntegrityReportModal report={sampleReport} />,
      harness.controller,
    );

    expect((markup.match(/type="checkbox"/g) ?? []).length).toBe(2);
    expect(markup).toContain(
      'aria-label="settings.integrity.assetTypePrimaryMedia',
    );
  });

  test("renders asset type labels for known asset types", async () => {
    const harness = await checkedHarness(sampleReport);
    const markup = markupOf(
      <IntegrityReportModal report={sampleReport} />,
      harness.controller,
    );

    expect(markup).toContain("settings.integrity.assetTypePrimaryMedia");
    expect(markup).toContain("settings.integrity.assetTypeCdg");
    expect(markup).toContain("settings.integrity.assetTypeStemVocals");
  });

  test("renders orphaned file paths", async () => {
    const harness = await checkedHarness(sampleReport);
    const markup = markupOf(
      <IntegrityReportModal report={sampleReport} />,
      harness.controller,
    );

    expect(markup).toContain("stems/orphan1.wav");
    expect(markup).toContain("stems/orphan2.wav");
  });

  test("renders remove-selected and close buttons", async () => {
    const harness = await checkedHarness(sampleReport);
    const markup = markupOf(
      <IntegrityReportModal report={sampleReport} />,
      harness.controller,
    );

    expect(markup).toContain("settings.integrity.removeSelected");
    expect(markup).toContain("common.close");
  });

  test("renders the skipped notice after a cleanup skipped entries", async () => {
    const harness = createSettingsHarness({
      overrides: {
        library: {
          checkLibraryIntegrity: async () => sampleReport,
          removeMissingLibraryEntries: async () => ({
            deleted_song_hashes: ["hash-aaaaaaaa"],
            skipped_song_hashes: ["a", "b", "c"],
          }),
        },
      },
    });
    await harness.controller.library.checkIntegrity();
    await harness.controller.library.cleanUpIntegrity();

    const markup = markupOf(
      <IntegrityReportModal report={sampleReport} />,
      harness.controller,
    );

    expect(markup).toContain("settings.integrity.skippedNotice");
    expect(markup).toContain("count=3");
  });

  test("renders no skipped notice when nothing was skipped", async () => {
    const harness = createSettingsHarness({
      overrides: {
        library: {
          checkLibraryIntegrity: async () => sampleReport,
          removeMissingLibraryEntries: async () => ({
            deleted_song_hashes: ["hash-aaaaaaaa"],
            skipped_song_hashes: [],
          }),
        },
      },
    });
    await harness.controller.library.checkIntegrity();
    await harness.controller.library.cleanUpIntegrity();

    const markup = markupOf(
      <IntegrityReportModal report={sampleReport} />,
      harness.controller,
    );

    expect(markup).not.toContain("settings.integrity.skippedNotice");
  });

  test("renders song hash prefix in issue rows", async () => {
    const harness = await checkedHarness(sampleReport);
    const markup = markupOf(
      <IntegrityReportModal report={sampleReport} />,
      harness.controller,
    );

    expect(markup).toContain("hash-aa");
    expect(markup).toContain("hash-bb");
  });

  test("falls back to raw asset type label for unknown asset types", async () => {
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
    const harness = await checkedHarness(report);
    const markup = markupOf(
      <IntegrityReportModal report={report} />,
      harness.controller,
    );

    expect(markup).toContain("custom_sidecar");
    expect(markup).toContain("extra/file.bin");
  });

  test("renders empty-path placeholder for issues without a path", async () => {
    const report: IntegrityReport = {
      ...emptyReport,
      missing_optional_assets: [
        { song_hash: "hash-nopath", asset_type: "cdg", path: "" },
      ],
    };
    const harness = await checkedHarness(report);
    const markup = markupOf(
      <IntegrityReportModal report={report} />,
      harness.controller,
    );

    expect(markup).toContain("settings.integrity.emptyPath");
  });

  test("shows deleting label while integrity cleanup is in progress", async () => {
    const harness = createSettingsHarness({
      overrides: {
        library: {
          checkLibraryIntegrity: async () => sampleReport,
          removeMissingLibraryEntries: () => new Promise(() => {}),
        },
      },
    });
    await harness.controller.library.checkIntegrity();
    void harness.controller.library.cleanUpIntegrity();

    const markup = markupOf(
      <IntegrityReportModal report={sampleReport} />,
      harness.controller,
    );

    expect(markup).toContain("common.deleting");
    expect(markup).toContain("disabled");
  });
});
