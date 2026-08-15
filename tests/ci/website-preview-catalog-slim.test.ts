import { describe, expect, test } from "vitest";
import {
  isPreviewCatalogModule,
  slimPreviewCatalogSource,
} from "../../website/src/slim-preview-catalog";

describe("preview catalog slim", () => {
  test("matches the generated catalog even with a Vite query suffix", () => {
    expect(isPreviewCatalogModule("/repo/src/mock/preview-songs.ts")).toBe(
      true,
    );
    expect(isPreviewCatalogModule("/repo/src/mock/preview-songs.ts?t=1")).toBe(
      true,
    );
    expect(
      isPreviewCatalogModule(String.raw`C:\repo\src\mock\preview-songs.ts`),
    ).toBe(true);
    expect(isPreviewCatalogModule("/repo/src/lib/i18n.ts")).toBe(false);
  });

  test("strips inline cover art and raw lyrics payloads", () => {
    const slimed = slimPreviewCatalogSource(`
      cover_art_base64: "/9j/4AAQSkZJRg==",
      raw_lrc: '[00:00.39] For real\\n[00:05.79] this time',
      raw_ttml: 'keep',
    `);
    expect(slimed).toContain('cover_art_base64: ""');
    expect(slimed).toContain("raw_lrc: ''");
    expect(slimed).toContain("raw_ttml: 'keep'");
    expect(slimed).not.toContain("/9j/");
    expect(slimed).not.toContain("For real");
  });

  test("strips a single-quoted TTML payload that contains escaped quotes", () => {
    const slimed = slimPreviewCatalogSource(
      "raw_lrc: '<tt>you\\'ll ever know</tt>',",
    );
    expect(slimed).toBe("raw_lrc: '',");
  });
});
