import { readdirSync, readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const DEFAULT_MIN_LINES = 18;
const DEFAULT_MAX_BLOCK_LINES = 25;
const DEFAULT_MAX_DUPLICATION_PERCENT = 1.0;

const EXCLUDED_DIRS = new Set(["node_modules", "target", "gen", "mock"]);
const EXCLUDED_RELATIVE_PATHS = new Set([
  "src/lib/han-pinyin-initials.ts",
  "src/types/ipc.ts",
]);

/**
 * Normalize source text into substantive lines with 1-based line numbers.
 * Strips comments, imports, module headers, Rust inline test modules, and
 * delimiter-only lines so clone detection focuses on executable logic.
 *
 * @param {string} source
 * @param {{ isRust?: boolean }} [options]
 * @returns {{ text: string; origLine: number }[]}
 */
export function normalizeSourceLines(source, options = {}) {
  let content = source;
  if (options.isRust) {
    const cfgTestIndex = content.search(/\n#\[cfg\(test\)\]/);
    if (cfgTestIndex !== -1) {
      content = content.slice(0, cfgTestIndex);
    }
  }

  content = content.replace(/\/\*[\s\S]*?\*\//g, (match) =>
    "\n".repeat((match.match(/\n/g) || []).length),
  );

  const rawLines = content.split(/\r?\n/);
  const normalized = [];

  for (let index = 0; index < rawLines.length; index += 1) {
    const line = rawLines[index]
      .replace(/\/\/.*$/, "")
      .trim()
      .replace(/\s+/g, " ");

    if (!line || line.length < 12) continue;
    if (
      /^(import |export type |export interface |use |pub use |mod |pub mod )/.test(
        line,
      )
    ) {
      continue;
    }
    if (/^[{}()[\];,]+$/.test(line)) continue;

    normalized.push({ text: line, origLine: index + 1 });
  }

  return normalized;
}

/**
 * Find contiguous duplicated blocks across normalized source files.
 *
 * @param {{ file: string; content: string; isRust?: boolean }[]} entries
 * @param {{ minLines?: number }} [options]
 */
export function findDuplicateBlocks(entries, options = {}) {
  const minLines = options.minLines ?? DEFAULT_MIN_LINES;
  const seenWindows = new Map();
  const rawMatches = [];
  const duplicatedLineKeys = new Set();
  let totalLines = 0;

  for (const entry of entries) {
    const isRust = entry.isRust ?? entry.file.endsWith(".rs");
    const lines = normalizeSourceLines(entry.content, { isRust });
    totalLines += lines.length;

    for (let i = 0; i + minLines <= lines.length; i += 1) {
      const fingerprint = lines
        .slice(i, i + minLines)
        .map((item) => item.text)
        .join("\n");
      const previous = seenWindows.get(fingerprint);

      if (previous) {
        rawMatches.push({
          fileA: previous.file,
          lineA: previous.origLine,
          idxA: previous.idx,
          fileB: entry.file,
          lineB: lines[i].origLine,
          idxB: i,
        });
        for (let offset = 0; offset < minLines; offset += 1) {
          duplicatedLineKeys.add(`${previous.file}:${previous.idx + offset}`);
          duplicatedLineKeys.add(`${entry.file}:${i + offset}`);
        }
      } else {
        seenWindows.set(fingerprint, {
          file: entry.file,
          origLine: lines[i].origLine,
          idx: i,
        });
      }
    }
  }

  const groupedByPair = new Map();
  for (const match of rawMatches) {
    const pairKey = `${match.fileA}->${match.fileB}`;
    const blocks = groupedByPair.get(pairKey) ?? [];
    const previousBlock = blocks[blocks.length - 1];

    if (
      previousBlock &&
      match.idxA === previousBlock.endIdxA + 1 &&
      match.idxB === previousBlock.endIdxB + 1
    ) {
      previousBlock.endIdxA = match.idxA;
      previousBlock.endIdxB = match.idxB;
      previousBlock.lines += 1;
    } else {
      blocks.push({
        fileA: match.fileA,
        lineA: match.lineA,
        endIdxA: match.idxA,
        fileB: match.fileB,
        lineB: match.lineB,
        endIdxB: match.idxB,
        lines: minLines,
      });
    }
    groupedByPair.set(pairKey, blocks);
  }

  const blocks = [...groupedByPair.values()]
    .flat()
    .map(({ fileA, lineA, fileB, lineB, lines }) => ({
      fileA,
      lineA,
      fileB,
      lineB,
      lines,
    }))
    .sort((left, right) => right.lines - left.lines);

  const duplicatedLines = duplicatedLineKeys.size;
  const duplicationPercent =
    totalLines === 0
      ? 0
      : Number(((duplicatedLines / totalLines) * 100).toFixed(2));
  const maxBlockLines = blocks.length > 0 ? blocks[0].lines : 0;

  return {
    fileCount: entries.length,
    totalLines,
    duplicatedLines,
    duplicationPercent,
    maxBlockLines,
    blocks,
  };
}

/**
 * @param {string} dir
 * @param {string[]} extensions
 * @param {string} rootDir
 * @param {string[]} [results]
 * @returns {string[]}
 */
function walkProductionFiles(dir, extensions, rootDir, results = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const fullPath = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (EXCLUDED_DIRS.has(entry.name)) continue;
      walkProductionFiles(fullPath, extensions, rootDir, results);
      continue;
    }
    if (!extensions.some((ext) => entry.name.endsWith(ext))) continue;
    if (/\.(test|spec)\.(ts|tsx)$/.test(entry.name)) continue;
    if (entry.name.endsWith("_tests.rs") || entry.name === "tests.rs") continue;

    const relPath = relative(rootDir, fullPath).replaceAll("\\", "/");
    if (EXCLUDED_RELATIVE_PATHS.has(relPath)) continue;
    results.push(relPath);
  }
  return results.sort();
}

/**
 * Scan the repository's TypeScript and Rust production sources for duplication.
 *
 * @param {string} rootDir
 * @param {{ minLines?: number; maxBlockLines?: number; maxDuplicationPercent?: number }} [thresholds]
 */
export function checkRepositoryDuplication(rootDir, thresholds = {}) {
  const minLines = thresholds.minLines ?? DEFAULT_MIN_LINES;
  const maxBlockLines = thresholds.maxBlockLines ?? DEFAULT_MAX_BLOCK_LINES;
  const maxDuplicationPercent =
    thresholds.maxDuplicationPercent ?? DEFAULT_MAX_DUPLICATION_PERCENT;

  const files = [
    ...walkProductionFiles(join(rootDir, "src"), [".ts", ".tsx"], rootDir),
    ...walkProductionFiles(join(rootDir, "src-tauri/src"), [".rs"], rootDir),
    ...walkProductionFiles(join(rootDir, "src-tauri/crates"), [".rs"], rootDir),
  ];

  const entries = files.map((file) => ({
    file,
    content: readFileSync(join(rootDir, file), "utf8"),
    isRust: file.endsWith(".rs"),
  }));

  const report = findDuplicateBlocks(entries, { minLines });
  const violations = [];

  for (const block of report.blocks) {
    if (block.lines > maxBlockLines) {
      violations.push(
        `Duplicate block of ${block.lines} substantive lines exceeds max ${maxBlockLines}: ${block.fileA}:${block.lineA} <-> ${block.fileB}:${block.lineB}`,
      );
    }
  }

  if (report.duplicationPercent > maxDuplicationPercent) {
    violations.push(
      `Overall duplication ${report.duplicationPercent}% exceeds max ${maxDuplicationPercent}%`,
    );
  }

  return {
    ok: violations.length === 0,
    report,
    violations,
  };
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const rootDir = resolve(fileURLToPath(new URL("..", import.meta.url)));
  const result = checkRepositoryDuplication(rootDir);
  const { fileCount, totalLines, duplicatedLines, duplicationPercent, blocks } =
    result.report;

  console.log(
    `Duplication check: ${fileCount} files, ${totalLines} substantive lines, ${duplicatedLines} duplicated (${duplicationPercent}%), ${blocks.length} clone block(s).`,
  );

  if (!result.ok) {
    for (const violation of result.violations) {
      console.error(`Duplication check failed: ${violation}`);
    }
    process.exit(1);
  }
}
