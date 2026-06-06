#!/usr/bin/env node

/**
 * Regenerate docs/generated/db-schema.md from src-tauri/migrations/*.sql.
 *
 * Usage:
 *   node scripts/generate-db-schema.mjs
 *
 * Idempotent: writing the same SQL input produces the same doc output.
 * Run after creating or altering a migration file so the checked-in
 * schema summary stays current.
 *
 * Conventions this script expects:
 *   - Migration files are named NNN_description.sql (e.g. 001_init.sql).
 *   - CREATE TABLE … AS … is not used; each CREATE TABLE is a full
 *     definition.
 *   - ALTER TABLE … ADD COLUMN statements name the table they modify.
 *   - Lines starting with "--" are treated as comments and ignored
 *     during column extraction.
 */

import { readFileSync, writeFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, "..");
const MIGRATIONS_DIR = join(REPO_ROOT, "src-tauri", "migrations");
const OUTPUT_FILE = join(REPO_ROOT, "docs", "generated", "db-schema.md");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Parse all CREATE TABLE statements from a migration file, returning
 * { tableName, columnDefs } pairs. Handles:
 *   - Multiple CREATE TABLEs per file
 *   - Comments containing `(` before the actual CREATE TABLE body
 *   - Column defs split across lines
 */
function parseCreateTableStatements(sql) {
  const results = [];
  // Match each CREATE TABLE … ( … ) group, with a simplified outer-paren matcher.
  const tableRe = /CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?(\w+)\s*\(/gi;
  let tableMatch;
  while ((tableMatch = tableRe.exec(sql)) !== null) {
    const tableName = tableMatch[1];
    const startPos = tableMatch.index + tableMatch[0].length;

    // Skip past inline comments and string content before the real `)`.
    let depth = 0;
    let i;
    for (i = startPos; i < sql.length; i++) {
      const ch = sql[i];
      if (ch === "(") depth++;
      else if (ch === ")") {
        if (depth === 0) break;
        depth--;
      }
      // Skip single-line SQL comments (-- ...) so `(` inside comments
      // does not confuse the depth tracker.
      if (ch === "-" && sql[i + 1] === "-") {
        while (i < sql.length && sql[i] !== "\n") i++;
        continue;
      }
    }

    // The body is between the opening `(` and the closing `)`
    const body = sql.slice(startPos, i);

    // Parse column definitions from the body
    const columns = [];
    for (let raw of body.split(",")) {
      raw = raw.trim();
      if (!raw) continue;
      // Skip constraints, primary key, foreign key, unique etc.
      if (/^(PRIMARY\s+KEY|FOREIGN\s+KEY|UNIQUE|CHECK|CONSTRAINT)\b/i.test(raw))
        continue;
      const parts = raw.split(/\s+/);
      if (parts.length < 2) continue;
      const colName = parts[0].replace(/`|"/g, "");
      const colType = parts[1].replace(/\(.*/, "").toUpperCase();
      let notes = "";
      if (/\bPRIMARY\s+KEY\b/i.test(raw)) notes = "Primary key";
      else if (/\bNOT\s+NULL\b/i.test(raw)) {
        notes = "NOT NULL";
        if (/\bDEFAULT\b/i.test(raw)) {
          const def = raw.match(/DEFAULT\s+(\S+)/i);
          if (def) notes += `, default ${def[1]}`;
        }
      }
      const rest = raw
        .replace(colName, "")
        .replace(parts[1], "")
        .replace(/\s+/g, " ")
        .trim();
      const ref = rest.match(/REFERENCES\s+(\w+)\s*\((\w+)\)/i);
      if (ref) {
        notes = `FK → ${ref[1]}(${ref[2]})`;
      }
      columns.push({ name: colName, type: colType, notes });
    }

    results.push({ tableName, columns });
  }
  return results;
}

/** Parse ALTER TABLE … ADD COLUMN statements. */
function extractAlterAdds(sql) {
  const results = [];
  const re =
    /ALTER\s+TABLE\s+(\w+)\s+ADD\s+(?:COLUMN\s+)?(\w+)\s+(\w+(?:\([^)]*\))?)/gi;
  let m;
  while ((m = re.exec(sql)) !== null) {
    let notes = "";
    // Check for DEFAULT after column type
    const remainder = sql.slice(m.index + m[0].length).trim();
    const def = remainder.match(/^DEFAULT\s+(\S+)/i);
    if (def) notes = `default ${def[1]}`;
    results.push({ table: m[1], name: m[2], type: m[3].toUpperCase(), notes });
  }
  return results;
}

/** Format a markdown table row for a column. */
function columnRow(col) {
  return `| \`${col.name}\` | \`${col.type}\` | ${col.notes} |`;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

const files = readdirSync(MIGRATIONS_DIR)
  .filter((f) => f.endsWith(".sql"))
  .sort();

// Accumulate schema state by applying migrations in order.
// tables: Map<tableName, { createdBy: string, columns: Array }>
const tables = new Map();

for (const file of files) {
  const sql = readFileSync(join(MIGRATIONS_DIR, file), "utf-8");
  const fileLabel = file.replace(".sql", "");

  // CREATE TABLE (supports multiple tables per file)
  for (const { tableName, columns } of parseCreateTableStatements(sql)) {
    tables.set(tableName, { createdBy: fileLabel, columns });
  }

  // ALTER TABLE … ADD COLUMN
  for (const add of extractAlterAdds(sql)) {
    if (!tables.has(add.table)) {
      // This shouldn't happen in a well-ordered migration set, but handle gracefully.
      tables.set(add.table, { createdBy: fileLabel, columns: [] });
    }
    tables.get(add.table).columns.push({
      name: add.name,
      type: add.type,
      notes: add.notes,
    });
  }
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

const lines = [];
lines.push("# Database Schema");
lines.push("");
lines.push(
  "This document is **auto-generated** from `src-tauri/migrations/*.sql` by `scripts/generate-db-schema.mjs`.",
);
lines.push(
  "Do **not** edit it by hand. Regenerate after any migration change:",
);
lines.push("");
lines.push("```bash");
lines.push("node scripts/generate-db-schema.mjs");
lines.push("```");
lines.push("");

const migrationFiles = files.map((f) => `\`${f}\``);
lines.push(`Source migrations: ${migrationFiles.join(", ")}.`);
lines.push("");

for (const [tableName, info] of tables) {
  lines.push(`## \`${tableName}\``);
  lines.push("");
  lines.push(`Created by \`${info.createdBy}.sql\`.`);
  lines.push("");
  lines.push("| Column | Type | Notes |");
  lines.push("| ------ | ---- | ----- |");
  for (const col of info.columns) {
    lines.push(columnRow(col));
  }
  lines.push("");
}

// Migration history summary
lines.push("## Migration History");
lines.push("");
for (let i = 0; i < files.length; i++) {
  const f = files[i];
  const sql = readFileSync(join(MIGRATIONS_DIR, f), "utf-8");
  const firstLine =
    sql
      .split("\n")[0]
      ?.replace(/^--\s*/, "")
      .trim() || "";
  lines.push(`${i + 1}. \`${f}\`${firstLine ? ` — ${firstLine}` : ""}`);
}
lines.push("");

const output = lines.join("\n");
writeFileSync(OUTPUT_FILE, output, "utf-8");
console.log(`Wrote ${OUTPUT_FILE}`);
