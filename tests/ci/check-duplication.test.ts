import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vite-plus/test";
import {
  checkRepositoryDuplication,
  findDuplicateBlocks,
  normalizeSourceLines,
} from "../../scripts/check-duplication.mjs";

describe("normalizeSourceLines", () => {
  test("strips comments, imports, and Rust inline test modules while keeping line numbers", () => {
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
      "    #[test]",
      "    fn duplicated_test_helper_is_ignored() {",
      "        let values = vec![1.0_f32, 2.0, 3.0];",
      "        assert_eq!(values.len(), 3);",
      "    }",
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
