import { existsSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vite-plus/test";
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
describe("website preview bundle contract", () => {
  test("aliases the romanizer to the preview stub", () => {
    expect(
      PREVIEW_ROMANIZER_MODULE_PATTERN.test("@/lib/lyrics-romanizer"),
    ).toBe(true);
    expect(
      PREVIEW_ROMANIZER_MODULE_PATTERN.test("@/lib/lyrics-romanizer/worker"),
    ).toBe(false);
  });

  test("aliases i18n to the landing locale pair", () => {
    expect(PREVIEW_I18N_MODULE_PATTERN.test("@/lib/i18n")).toBe(true);
    expect(PREVIEW_I18N_MODULE_PATTERN.test("@/lib/i18n/extra")).toBe(false);
  });

  test("strips inline catalog payloads that the preview never edits", () => {
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
    for (const modulePath of UNUSED_PREVIEW_MODULES) {
      const pattern = previewUnusedModulePattern(modulePath);
      expect(pattern.test(`@/${modulePath}`), modulePath).toBe(true);
      expect(pattern.test(`@/${modulePath}/extra`), modulePath).toBe(false);
    }
  });

  test("ships a JPEG cover asset for every preview song hash", () => {
    for (const song of PREVIEW_SONGS) {
      expect(
        existsSync(join(projectRoot, "src/mock/covers", `${song.hash}.jpg`)),
        song.hash,
      ).toBe(true);
    }
  });
});
