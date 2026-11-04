import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const ALL = ["actions", "csharp", "javascript-typescript", "python", "rust"];

export function classifyCodeql(files, event, author, branch) {
  if (event !== "pull_request") return ALL;
  if (
    author === "github-actions[bot]" &&
    branch.startsWith("release-please--branches--main--") &&
    files.every((file) =>
      [
        ".release-please-manifest.json",
        "CHANGELOG.md",
        "package.json",
      ].includes(file),
    )
  )
    return [];
  if (files.some((file) => file.startsWith(".github/workflows/"))) return ALL;
  const code = files.filter((file) => !file.endsWith(".md"));
  return ALL.filter((language) => {
    if (language === "javascript-typescript")
      return code.some((file) =>
        /\.(js|jsx|ts|tsx|mjs|cjs)$|(^|\/)(package\.json|pnpm-lock\.yaml)$/.test(
          file,
        ),
      );
    if (language === "rust")
      return code.some((file) => /\.rs$|(^|\/)Cargo\.(toml|lock)$/.test(file));
    if (language === "python") return code.some((file) => file.endsWith(".py"));
    if (language === "csharp")
      return code.some((file) => /\.(cs|csproj|sln)$/.test(file));
    return false;
  });
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const files = readFileSync(process.argv[2], "utf8")
    .split("\n")
    .filter(Boolean);
  console.log(
    JSON.stringify(
      classifyCodeql(
        files,
        process.env.EVENT,
        process.env.AUTHOR,
        process.env.BRANCH,
      ),
    ),
  );
}
