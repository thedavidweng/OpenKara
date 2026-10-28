import { describe, expect, test } from "vite-plus/test";
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
});
