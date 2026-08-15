import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import { PREVIEW_SONGS } from "../../src/mock/preview-songs";
import {
  isPreviewCatalogModule,
  slimPreviewCatalogSource,
} from "../../website/src/slim-preview-catalog";
import {
  PREVIEW_I18N_MODULE_PATTERN,
  PREVIEW_ROMANIZER_MODULE_PATTERN,
  UNUSED_PREVIEW_MODULES,
  previewUnusedModulePattern,
} from "../../website/src/preview-aliases";

const projectRoot = fileURLToPath(new URL("../..", import.meta.url));
const websiteVite = readFileSync(
  join(projectRoot, "website/vite.config.ts"),
  "utf8",
);

describe("website preview bundle contract", () => {
  test("aliases the romanizer to the preview stub", () => {
    expect(websiteVite).toContain("preview-romanizer.ts");
    expect(websiteVite).toContain("PREVIEW_ROMANIZER_MODULE_PATTERN");
    expect(
      PREVIEW_ROMANIZER_MODULE_PATTERN.test("@/lib/lyrics-romanizer"),
    ).toBe(true);
    expect(
      PREVIEW_ROMANIZER_MODULE_PATTERN.test("@/lib/lyrics-romanizer/worker"),
    ).toBe(false);
  });

  test("aliases i18n to the landing locale pair", () => {
    expect(websiteVite).toContain("preview-i18n.ts");
    expect(websiteVite).toContain("PREVIEW_I18N_MODULE_PATTERN");
    expect(PREVIEW_I18N_MODULE_PATTERN.test("@/lib/i18n")).toBe(true);
    expect(PREVIEW_I18N_MODULE_PATTERN.test("@/lib/i18n/extra")).toBe(false);
  });

  test("strips inline catalog payloads that the preview never edits", () => {
    expect(websiteVite).toContain("slimPreviewCatalogPlugin");
    expect(isPreviewCatalogModule("/repo/src/mock/preview-songs.ts")).toBe(
      true,
    );
    const slimed = slimPreviewCatalogSource(
      'cover_art_base64: "/9j/4AAQ", raw_lrc: "[00:00.00] keep-me",',
    );
    expect(slimed).toContain('cover_art_base64: ""');
    expect(slimed).toContain('raw_lrc: ""');
    expect(slimed).not.toContain("/9j/");
    expect(slimed).not.toContain("keep-me");
  });

  test("matches unused preview surfaces on their exact module ids", () => {
    expect(websiteVite).toContain("preview-unused.ts");
    expect(websiteVite).toContain("previewUnusedModulePattern");
    for (const modulePath of UNUSED_PREVIEW_MODULES) {
      const pattern = previewUnusedModulePattern(modulePath);
      expect(pattern.test(`@/${modulePath}`), modulePath).toBe(true);
      expect(pattern.test(`@/${modulePath}/extra`), modulePath).toBe(false);
    }
  });

  test("ships a JPEG cover asset for every preview song hash", () => {
    const previewCovers = readFileSync(
      join(projectRoot, "website/src/preview-covers.ts"),
      "utf8",
    );
    const appPreview = readFileSync(
      join(projectRoot, "website/src/AppPreview.tsx"),
      "utf8",
    );
    expect(previewCovers).toContain("../../src/mock/covers/*.jpg");
    expect(appPreview).toContain("coverArtUrls: PREVIEW_COVER_URLS");
    for (const song of PREVIEW_SONGS) {
      expect(
        existsSync(join(projectRoot, "src/mock/covers", `${song.hash}.jpg`)),
        song.hash,
      ).toBe(true);
    }
  });
});
