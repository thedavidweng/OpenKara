import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const projectRoot = fileURLToPath(new URL("../..", import.meta.url));
const lefthook = readFileSync(join(projectRoot, "lefthook.yml"), "utf8");

describe("lefthook gates", () => {
  test("pre-commit and pre-push both run knip", () => {
    expect(lefthook).toMatch(
      /pre-commit:[\s\S]*?knip:[\s\S]*?pnpm knip --no-progress/,
    );
    expect(lefthook).toMatch(
      /pre-push:[\s\S]*?knip:[\s\S]*?pnpm knip --no-progress/,
    );
  });
});
