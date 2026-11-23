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

function countNewlines(text) {
  let count = 0;
  for (let i = 0; i < text.length; i += 1) {
    if (text[i] === "\n") count += 1;
  }
  return count;
}

function findQuotedEnd(source, start, quote, allowMultiline) {
  let pos = start + 1;
  while (pos < source.length) {
    const ch = source[pos];
    if (ch === "\\") {
      pos += 2;
      continue;
    }
    if (ch === quote) return pos + 1;
    if (!allowMultiline && ch === "\n") return pos;
    pos += 1;
  }
  return source.length;
}

function findRustRawStringEnd(source, pos) {
  if (pos > 0 && /[A-Za-z0-9_]/.test(source[pos - 1])) return -1;
  const prefix = /^b?r(#*)"/.exec(source.slice(pos));
  if (!prefix) return -1;
  const closing = `"${prefix[1]}`;
  const closeIdx = source.indexOf(closing, pos + prefix[0].length);
  return closeIdx === -1 ? source.length : closeIdx + closing.length;
}

function tryStringLiteralEnd(source, pos, isRust) {
  const ch = source[pos];
  if (isRust && (ch === "r" || ch === "b")) {
    return findRustRawStringEnd(source, pos);
  }
  if (ch === '"') return findQuotedEnd(source, pos, '"', true);
  if (!isRust && ch === "`") return findQuotedEnd(source, pos, "`", true);
  if (ch === "'") {
    if (!isRust) return findQuotedEnd(source, pos, "'", false);
    const charMatch = /^'(?:\\[^'\n]+|[^'\\\n])'/.exec(
      source.slice(pos, pos + 16),
    );
    return charMatch ? pos + charMatch[0].length : -1;
  }
  return -1;
}

function stripCommentsPreservingStrings(source, isRust) {
  let out = "";
  let pos = 0;

  while (pos < source.length) {
    const strEnd = tryStringLiteralEnd(source, pos, isRust);
    if (strEnd !== -1) {
      out += source.slice(pos, strEnd);
      pos = strEnd;
      continue;
    }

    if (source[pos] === "/" && source[pos + 1] === "/") {
      const lineEnd = source.indexOf("\n", pos + 2);
      if (lineEnd === -1) break;
      pos = lineEnd;
      continue;
    }

    if (source[pos] === "/" && source[pos + 1] === "*") {
      const closeIdx = source.indexOf("*/", pos + 2);
      const end = closeIdx === -1 ? source.length : closeIdx + 2;
      out += "\n".repeat(countNewlines(source.slice(pos, end)));
      pos = end;
      continue;
    }

    out += source[pos];
    pos += 1;
  }

  return out;
}

function skipBalancedBracketAttr(source, pos) {
  let depth = 0;
  let cursor = pos;
  while (cursor < source.length) {
    const strEnd = tryStringLiteralEnd(source, cursor, true);
    if (strEnd !== -1) {
      cursor = strEnd;
      continue;
    }
    if (source[cursor] === "[") depth += 1;
    else if (source[cursor] === "]") {
      depth -= 1;
      if (depth <= 0) return cursor + 1;
    }
    cursor += 1;
  }
  return source.length;
}

function findRustItemEnd(source, attrStart) {
  let cursor = attrStart;
  while (cursor < source.length) {
    while (cursor < source.length && /\s/.test(source[cursor])) cursor += 1;
    if (source[cursor] === "#" && source[cursor + 1] === "[") {
      cursor = skipBalancedBracketAttr(source, cursor + 1);
      continue;
    }
    break;
  }

  let braceDepth = 0;
  while (cursor < source.length) {
    const strEnd = tryStringLiteralEnd(source, cursor, true);
    if (strEnd !== -1) {
      cursor = strEnd;
      continue;
    }
    const ch = source[cursor];
    if (ch === "{") {
      braceDepth += 1;
    } else if (ch === "}") {
      braceDepth -= 1;
      if (braceDepth === 0) return cursor + 1;
    } else if (ch === ";" && braceDepth === 0) {
      return cursor + 1;
    }
    cursor += 1;
  }
  return source.length;
}

function stripRustCfgTestItems(source) {
  let out = "";
  let pos = 0;

  while (pos < source.length) {
    const strEnd = tryStringLiteralEnd(source, pos, true);
    if (strEnd !== -1) {
      out += source.slice(pos, strEnd);
      pos = strEnd;
      continue;
    }

    if (
      source[pos] === "#" &&
      /^#\[\s*cfg\s*\(\s*test\s*\)\s*\]/.test(source.slice(pos, pos + 40))
    ) {
      const itemEnd = findRustItemEnd(source, pos);
      out += "\n".repeat(countNewlines(source.slice(pos, itemEnd)));
      pos = itemEnd;
      continue;
    }

    out += source[pos];
    pos += 1;
  }

  return out;
}

/**
 * Normalize source text into substantive lines with 1-based line numbers.
 * Strips comments (preserving string literals), imports, module headers,
 * Rust `#[cfg(test)]` items, and delimiter-only lines.
 *
 * @param {string} source
 * @param {{ isRust?: boolean }} [options]
 * @returns {{ text: string; origLine: number }[]}
 */
export function normalizeSourceLines(source, options = {}) {
  const isRust = Boolean(options.isRust);
  let content = stripCommentsPreservingStrings(source, isRust);
  if (isRust) {
    content = stripRustCfgTestItems(content);
  }

  const rawLines = content.split(/\r?\n/);
  const normalized = [];

  for (let index = 0; index < rawLines.length; index += 1) {
    const line = rawLines[index].trim().replace(/\s+/g, " ");
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

function mergeDiagonalBlocks(rawMatches, minLines) {
  const byDiagonal = new Map();
  for (const match of rawMatches) {
    const diagKey = `${match.fileA}->${match.fileB}:${match.idxB - match.idxA}`;
    const list = byDiagonal.get(diagKey) ?? [];
    list.push(match);
    byDiagonal.set(diagKey, list);
  }

  const merged = [];
  for (const matches of byDiagonal.values()) {
    matches.sort((a, b) => a.idxB - b.idxB);
    let current = null;
    for (const match of matches) {
      if (current && match.idxB === current.endIdxB + 1) {
        current.endIdxA = match.idxA;
        current.endIdxB = match.idxB;
        current.lines += 1;
      } else {
        if (current) merged.push(current);
        current = {
          fileA: match.fileA,
          lineA: match.lineA,
          endIdxA: match.idxA,
          fileB: match.fileB,
          lineB: match.lineB,
          endIdxB: match.idxB,
          lines: minLines,
        };
      }
    }
    if (current) merged.push(current);
  }

  return merged
    .map(({ fileA, lineA, fileB, lineB, lines }) => ({
      fileA,
      lineA,
      fileB,
      lineB,
      lines,
    }))
    .sort((left, right) => right.lines - left.lines);
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
      const previousList = seenWindows.get(fingerprint);

      if (previousList) {
        for (const previous of previousList) {
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
        }
        previousList.push({
          file: entry.file,
          origLine: lines[i].origLine,
          idx: i,
        });
      } else {
        seenWindows.set(fingerprint, [
          {
            file: entry.file,
            origLine: lines[i].origLine,
            idx: i,
          },
        ]);
      }
    }
  }

  const blocks = mergeDiagonalBlocks(rawMatches, minLines);
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
