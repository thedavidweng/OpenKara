import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("../..", import.meta.url));
const siteCss = readFileSync(join(projectRoot, "website/src/site.css"), "utf8");

describe("website preview pointer contract", () => {
  test("song-switch buttons stay clickable in the playlist-only preview", () => {
    const blocklistStart = siteCss.indexOf(
      "button:not([data-preview-playlist-switch",
    );
    expect(blocklistStart).toBeGreaterThan(-1);
    const blocklist = siteCss.slice(blocklistStart, blocklistStart + 500);
    expect(blocklist).toContain(':not([data-preview-song-switch="true"])');
    expect(blocklist).toContain("pointer-events: none");
  });
});
