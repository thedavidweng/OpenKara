import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("../..", import.meta.url));
const websiteVite = readFileSync(
  join(projectRoot, "website/vite.config.ts"),
  "utf8",
);
const catalogSlim = readFileSync(
  join(projectRoot, "website/src/slim-preview-catalog.ts"),
  "utf8",
);

describe("website preview bundle contract", () => {
  test("aliases the romanizer to the preview stub", () => {
    expect(websiteVite).toContain("preview-romanizer.ts");
    expect(websiteVite).toContain("@\\/lib\\/lyrics-romanizer");
  });

  test("aliases i18n to the landing locale pair", () => {
    expect(websiteVite).toContain("preview-i18n.ts");
    expect(websiteVite).toContain("@\\/lib\\/i18n");
  });

  test("strips inline catalog payloads that the preview never edits", () => {
    expect(websiteVite).toContain("slim-preview-catalog");
    expect(catalogSlim).toContain("cover_art_base64");
    expect(catalogSlim).toContain("raw_lrc");
  });

  test("aliases unused preview surfaces to empty stubs", () => {
    expect(websiteVite).toContain("preview-unused.ts");
    expect(websiteVite).toContain("SettingsOverlay");
    expect(websiteVite).toContain("QueuePanel");
    expect(websiteVite).toContain("UpdateBanner");
    expect(websiteVite).toContain("escapeRegExpLiteral");
  });
});
