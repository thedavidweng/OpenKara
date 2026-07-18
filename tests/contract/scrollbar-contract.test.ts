import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

/**
 * Source/contract test for the platform scrollbar contract (#97).
 *
 * Protects the native-overlay decision for macOS from later global expansions:
 * no selector beginning with the mac platform marker may install author
 * scrollbar geometry (`::-webkit-scrollbar`), `scrollbar-width: thin`, or a
 * custom `scrollbar-color` value. macOS must keep `scrollbar-color/width: auto`
 * so WKWebView retains its native overlay/autohide behavior.
 *
 * This test reads the raw stylesheet from disk rather than importing it, so it
 * is unaffected by Vitest's CSS stubbing. It lives under `tests/contract/`
 * (outside `src/`) so the production `tsc` build does not type-check Node APIs.
 */

const GLOBALS_CSS = readFileSync(
  fileURLToPath(new URL("../../src/styles/globals.css", import.meta.url)),
  "utf8",
);

const MAC_SELECTOR = 'data-window-chrome-platform="mac"';

/** Find the body of the rule block whose selector contains `selectorOccurrence`. */
function blockBodyForSelectorAt(css: string, selectorIndex: number): string {
  const braceOpen = css.indexOf("{", selectorIndex);
  if (braceOpen === -1) {
    return "";
  }
  let depth = 1;
  let j = braceOpen + 1;
  while (j < css.length && depth > 0) {
    const c = css[j];
    if (c === "{") {
      depth++;
    } else if (c === "}") {
      depth--;
    }
    j++;
  }
  return css.slice(braceOpen + 1, j - 1);
}

describe("scrollbar platform contract", () => {
  test("mac selector blocks contain no author scrollbar geometry or custom colors", () => {
    const forbiddenGeometry = /::-webkit-scrollbar/;
    const forbiddenThinWidth = /scrollbar-width\s*:\s*thin/i;
    // Capture every scrollbar-color declaration value and require it to be
    // exactly "auto". A negative-lookahead regex would backtrack past the
    // optional whitespace, so capture-and-compare is the robust check.
    const scrollbarColorDecls = /scrollbar-color\s*:\s*([^;]+)/gi;

    let cursor = 0;
    let foundMacBlock = false;
    while (cursor < GLOBALS_CSS.length) {
      const idx = GLOBALS_CSS.indexOf(MAC_SELECTOR, cursor);
      if (idx === -1) {
        break;
      }
      const body = blockBodyForSelectorAt(GLOBALS_CSS, idx);
      foundMacBlock = true;

      expect(
        forbiddenGeometry.test(body),
        `mac selector block must not contain ::-webkit-scrollbar, found in: ${body}`,
      ).toBe(false);
      expect(
        forbiddenThinWidth.test(body),
        `mac selector block must not set scrollbar-width: thin, found in: ${body}`,
      ).toBe(false);
      const matches = [...body.matchAll(scrollbarColorDecls)];
      for (const match of matches) {
        expect(
          match[1].trim(),
          `mac selector block must not set a custom scrollbar-color (only auto is allowed), found in: ${body}`,
        ).toBe("auto");
      }

      cursor = idx + MAC_SELECTOR.length;
    }

    expect(
      foundMacBlock,
      "expected at least one mac platform selector block",
    ).toBe(true);
  });

  test("mac platform applies dark color-scheme and native auto scrollbars", () => {
    expect(GLOBALS_CSS).toContain(
      '[data-window-chrome-platform="mac"] {\n    color-scheme: dark;',
    );
    expect(GLOBALS_CSS).toContain(
      '[data-window-chrome-platform="mac"],\n  [data-window-chrome-platform="mac"] * {\n    scrollbar-color: auto;\n    scrollbar-width: auto;',
    );
  });

  test("desktop thin scrollbar is scoped only to the desktop marker", () => {
    const desktopThin =
      /@supports\s*\(scrollbar-color:\s*auto\)\s*\{[^}]*\[data-window-chrome-platform="desktop"\][^}]*scrollbar-width:\s*thin/s;
    expect(desktopThin.test(GLOBALS_CSS)).toBe(true);

    // The @supports not fallback must not be combined with the mac selector.
    // Use brace-depth counting (blockBodyForSelectorAt) rather than a
    // non-greedy regex, which would stop at the first inner rule's closing
    // brace and leave the rest of the @supports not block unchecked.
    const supportsNotIdx = GLOBALS_CSS.indexOf(
      "@supports not (scrollbar-color: auto)",
    );
    expect(
      supportsNotIdx,
      "expected a desktop-scoped @supports not fallback",
    ).not.toBe(-1);
    const fallbackBody = blockBodyForSelectorAt(GLOBALS_CSS, supportsNotIdx);
    expect(fallbackBody).not.toBe("");
    expect(fallbackBody).not.toContain('data-window-chrome-platform="mac"');
  });

  test("forced-colors returns scrollbar control to the system for every platform", () => {
    expect(GLOBALS_CSS).toContain("@media (forced-colors: active)");
    expect(GLOBALS_CSS).toContain("scrollbar-color: auto;");
    expect(GLOBALS_CSS).toContain("forced-color-adjust: auto;");
    expect(GLOBALS_CSS).toContain(
      "[data-window-chrome-platform],\n    [data-window-chrome-platform] *",
    );
  });

  test("semantic scrollbar tokens are defined at the application root", () => {
    expect(GLOBALS_CSS).toContain("--scrollbar-thumb: #6e6e73;");
    expect(GLOBALS_CSS).toContain("--scrollbar-thumb-hover: #8e8e93;");
    expect(GLOBALS_CSS).toContain("--scrollbar-thumb-active: #aeaeb2;");
    expect(GLOBALS_CSS).toContain("--scrollbar-track: transparent;");
  });
});
