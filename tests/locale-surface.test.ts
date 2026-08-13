import { describe, expect, test } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const CJK = /[\u4e00-\u9fff]/;
const ROOT = fileURLToPath(new URL("..", import.meta.url));

function read(relativePath: string): string {
  return readFileSync(join(ROOT, relativePath), "utf8");
}

function cjkLines(text: string): string[] {
  return text.split(/\r?\n/).filter((line) => CJK.test(line));
}

describe("locale surfaces stay in one language", () => {
  test("English README mixes Chinese only in the language switcher", () => {
    const lines = cjkLines(read("README.md"));
    expect(lines).toEqual(["[简体中文](./README_CN.md)"]);
  });

  test("English website and contributor docs have no Chinese", () => {
    const files = [
      "CONTRIBUTING.md",
      "SECURITY.md",
      "ARCHITECTURE.md",
      "website/index.md",
      "website/faq.md",
      "website/privacy.md",
      "website/terms.md",
    ];
    expect(files.flatMap((file) => cjkLines(read(file)))).toEqual([]);
  });

  test("English UI copy keeps CJK only as native language names", () => {
    const en = JSON.parse(read("src/locales/en.json")) as {
      languageNames: Record<string, string>;
    };
    const names = new Set(Object.values(en.languageNames));
    const leaked = cjkLines(read("src/locales/en.json")).filter((line) => {
      const quoted = line.match(/:\s*"([^"]+)"/);
      return quoted === null || !names.has(quoted[1]);
    });
    expect(leaked).toEqual([]);
  });

  test("Chinese help copy uses the localized debug-info menu name", () => {
    expect(read("README_CN.md")).not.toContain("Copy Debug Info");
    expect(read("website/zh/faq.md")).not.toContain("Copy Debug Info");
  });
});
