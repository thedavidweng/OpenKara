import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { JSDOM } from "jsdom";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("../..", import.meta.url));
const siteCss = readFileSync(join(projectRoot, "website/src/site.css"), "utf8");

interface CssRule {
  selector: string;
  body: string;
}

function cssRules(css: string): CssRule[] {
  const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const rules: CssRule[] = [];
  let index = 0;
  while (index < withoutComments.length) {
    const open = withoutComments.indexOf("{", index);
    if (open < 0) {
      break;
    }
    const selector = withoutComments.slice(index, open).trim();
    let depth = 1;
    let cursor = open + 1;
    while (cursor < withoutComments.length && depth > 0) {
      const ch = withoutComments[cursor];
      if (ch === "{") depth += 1;
      if (ch === "}") depth -= 1;
      cursor += 1;
    }
    const body = withoutComments.slice(open + 1, cursor - 1);
    if (selector.startsWith("@")) {
      rules.push(...cssRules(body));
    } else if (selector) {
      rules.push({ selector, body });
    }
    index = cursor;
  }
  return rules;
}

function splitSelectors(selectorList: string): string[] {
  const parts: string[] = [];
  let current = "";
  let depth = 0;
  for (const ch of selectorList) {
    if (ch === "(") depth += 1;
    if (ch === ")") depth -= 1;
    if (ch === "," && depth === 0) {
      if (current.trim()) parts.push(current.trim());
      current = "";
    } else {
      current += ch;
    }
  }
  if (current.trim()) parts.push(current.trim());
  return parts;
}

function compactSelector(selector: string): string {
  return selector.replace(/\s+/g, " ").trim();
}

function isPlaylistOnlyButtonSelector(selector: string): boolean {
  const compact = compactSelector(selector);
  return (
    compact.includes('[data-preview-interaction-mode="playlist-only"]') &&
    /(?:^| )button(?::|$)/.test(compact)
  );
}

function disablesPointerEvents(body: string): boolean {
  return /pointer-events\s*:\s*none/.test(body);
}

function excludesSongSwitch(selector: string): boolean {
  return /:not\(\s*\[data-preview-song-switch="true"\]\s*\)/.test(
    compactSelector(selector),
  );
}

function playlistOnlyButtonBlocklistSelectors(css: string): string[] {
  return cssRules(css).flatMap((rule) => {
    if (!disablesPointerEvents(rule.body)) {
      return [];
    }
    return splitSelectors(rule.selector).filter(isPlaylistOnlyButtonSelector);
  });
}

function playlistOnlyButtonBlocklistExcludesSongSwitch(css: string): boolean {
  const selectors = playlistOnlyButtonBlocklistSelectors(css);
  return selectors.length > 0 && selectors.every(excludesSongSwitch);
}

const LEAKY_CSS = `
.product-preview [data-preview-interaction-mode="playlist-only"]
  button:not([data-preview-playlist-switch="true"]) {
  pointer-events: none;
}
.unrelated button:not([data-preview-song-switch="true"]) {
  color: red;
}
.elsewhere { pointer-events: none; }
`;

describe("website preview pointer contract", () => {
  test("playlist-only button blocklist excludes song-switch rows", () => {
    expect(playlistOnlyButtonBlocklistExcludesSongSwitch(siteCss)).toBe(true);
  });

  test("song-switch exclusion on another rule does not satisfy the blocklist", () => {
    expect(playlistOnlyButtonBlocklistExcludesSongSwitch(LEAKY_CSS)).toBe(
      false,
    );
  });

  test("computed style keeps song-switch rows clickable and other preview buttons inert", () => {
    const { window } = new JSDOM(
      `<!doctype html>
      <html>
        <head><style>${siteCss}</style></head>
        <body>
          <div class="product-preview">
            <div data-preview-interaction-mode="playlist-only">
              <button type="button" data-preview-song-switch="true">One Last Kiss</button>
              <button type="button">Separate</button>
            </div>
          </div>
        </body>
      </html>`,
      { pretendToBeVisual: true },
    );
    const [songSwitch, blocked] = window.document.querySelectorAll("button");
    expect(window.getComputedStyle(songSwitch).pointerEvents).not.toBe("none");
    expect(window.getComputedStyle(blocked).pointerEvents).toBe("none");
  });
});
