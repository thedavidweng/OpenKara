import { readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

const [inputDir, outputPath] = process.argv.slice(2);
if (!inputDir || !outputPath) {
  throw new Error(
    "usage: node scripts/ci/merge-size-reports.mjs INPUT_DIR OUTPUT",
  );
}

const names = (
  await readdir(inputDir).catch((error) => {
    if (error.code === "ENOENT") return [];
    throw error;
  })
)
  .filter((name) => name.endsWith(".json"))
  .sort();
const reports = [];
for (const name of names) {
  const report = JSON.parse(await readFile(path.join(inputDir, name), "utf8"));
  if (report.schema_version !== 1 || report.status !== "measured") {
    throw new Error(`unsupported size report ${name}`);
  }
  reports.push(report);
}

await writeFile(
  outputPath,
  `${JSON.stringify({ schema_version: 1, status: "measured", reports }, null, 2)}\n`,
);
