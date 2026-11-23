import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vite-plus/test";
import {
  checkRepositoryDuplication,
  findDuplicateBlocks,
  normalizeSourceLines,
} from "../../scripts/check-duplication.mjs";

describe("normalizeSourceLines", () => {
  test("strips Rust test modules while preserving later production code and line numbers", () => {
    const rustSource = [
      "use std::collections::HashMap;",
      "// comment line",
      "pub fn compute_window(samples: &[f32]) -> f32 {",
      "    let sum: f32 = samples.iter().copied().sum();",
      "    sum / samples.len().max(1) as f32",
      "}",
      "",
      "#[cfg(test)]",
      "mod tests {",
      '    const FIXTURE: &str = "https://example.com/{brace}";',
      "    #[test]",
      "    fn duplicated_test_helper_is_ignored() {",
      "        let values = vec![1.0_f32, 2.0, 3.0];",
      "        assert_eq!(values.len(), 3);",
      "    }",
      "}",
      "",
      "pub fn post_test_production<'a>(label: &'a str) -> usize {",
      "    label.trim().len()",
      "}",
    ].join("\n");

    const lines = normalizeSourceLines(rustSource, { isRust: true });
    expect(lines).toEqual([
      { text: "pub fn compute_window(samples: &[f32]) -> f32 {", origLine: 3 },
      {
        text: "let sum: f32 = samples.iter().copied().sum();",
        origLine: 4,
      },
      { text: "sum / samples.len().max(1) as f32", origLine: 5 },
      {
        text: "pub fn post_test_production<'a>(label: &'a str) -> usize {",
        origLine: 18,
      },
      { text: "label.trim().len()", origLine: 19 },
    ]);
  });

  test("preserves URLs and comment-like text inside string literals while removing real comments", () => {
    const tsSource = [
      "/* leading block",
      "   comment */",
      'const endpointA = "https://alpha.example.com/v1/tracks"; // trailing comment',
      'const pattern = "/* keep inside string */";',
      "const template = `https://beta.example.com/${endpointA}`; /* inline block */",
    ].join("\n");

    expect(normalizeSourceLines(tsSource)).toEqual([
      {
        text: 'const endpointA = "https://alpha.example.com/v1/tracks";',
        origLine: 3,
      },
      { text: 'const pattern = "/* keep inside string */";', origLine: 4 },
      {
        text: "const template = `https://beta.example.com/${endpointA}`;",
        origLine: 5,
      },
    ]);
  });
});

describe("findDuplicateBlocks", () => {
  test("detects contiguous duplicated logic across files", () => {
    const sharedBody = [
      "const normalizedTitle = rawTitle.trim().toLowerCase();",
      "const normalizedArtist = rawArtist.trim().toLowerCase();",
      "if (!normalizedTitle || !normalizedArtist) return null;",
      "const compositeKey = `${normalizedArtist}::${normalizedTitle}`;",
      "return cacheMap.get(compositeKey) ?? null;",
    ].join("\n");

    const report = findDuplicateBlocks(
      [
        {
          file: "src/a.ts",
          content: `import { x } from "./x";\nexport function lookupA() {\n${sharedBody}\n}\n`,
        },
        {
          file: "src/b.ts",
          content: `// header\nexport function lookupB() {\n${sharedBody}\n}\n`,
        },
      ],
      { minLines: 5 },
    );

    expect(report.blocks).toEqual([
      {
        fileA: "src/a.ts",
        lineA: 3,
        fileB: "src/b.ts",
        lineB: 3,
        lines: 5,
      },
    ]);
    expect(report.duplicatedLines).toBe(10);
  });

  test("measures full 26-line B/C duplicate across three-file partial overlap and triggers maxBlockLines", () => {
    const tempRoot = mkdtempSync(join(os.tmpdir(), "openkara-dup-check-"));
    try {
      mkdirSync(join(tempRoot, "src"), { recursive: true });
      mkdirSync(join(tempRoot, "src-tauri/src"), { recursive: true });
      mkdirSync(join(tempRoot, "src-tauri/crates"), { recursive: true });

      const shared26 = Array.from(
        { length: 26 },
        (_, idx) =>
          `const sharedStepValue${idx} = computeNormalizedMetric(inputPayload, ${idx + 100});`,
      );
      const fileALines = shared26.slice(0, 18);
      const uniqueFiller = Array.from(
        { length: 8000 },
        (_, idx) =>
          `const distinctBaselineLine${idx} = transformUniqueRecord(sourceItems, ${idx + 1000});`,
      );

      writeFileSync(join(tempRoot, "src/a.ts"), fileALines.join("\n"), "utf8");
      writeFileSync(join(tempRoot, "src/b.ts"), shared26.join("\n"), "utf8");
      writeFileSync(join(tempRoot, "src/c.ts"), shared26.join("\n"), "utf8");
      writeFileSync(
        join(tempRoot, "src/filler.ts"),
        uniqueFiller.join("\n"),
        "utf8",
      );

      const result = checkRepositoryDuplication(tempRoot, {
        minLines: 18,
        maxBlockLines: 25,
        maxDuplicationPercent: 1.0,
      });

      expect(result.report.duplicationPercent).toBeLessThan(1.0);
      expect(result.report.maxBlockLines).toBe(26);
      expect(result.ok).toBe(false);
      expect(result.violations).toContain(
        "Duplicate block of 26 substantive lines exceeds max 25: src/b.ts:1 <-> src/c.ts:1",
      );
    } finally {
      rmSync(tempRoot, { recursive: true, force: true });
    }
  });
});

describe("checkRepositoryDuplication", () => {
  test("production sources stay within duplication thresholds", () => {
    const repoRoot = resolve(fileURLToPath(new URL("../..", import.meta.url)));
    const result = checkRepositoryDuplication(repoRoot);
    expect(result.violations).toEqual([]);
    expect(result.ok).toBe(true);
    expect(result.report.fileCount).toBeGreaterThan(100);
  });
});
