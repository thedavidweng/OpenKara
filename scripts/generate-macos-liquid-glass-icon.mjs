import { execFileSync } from "node:child_process";
import { cp, mkdir, rm } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const projectRoot = path.resolve(__dirname, "..");
const iconsDir = path.join(projectRoot, "src-tauri", "icons");
const iconComposerDir = path.join(iconsDir, "OpenKara.icon");
const stagingDir = path.join(iconsDir, ".liquid-glass-staging");

if (process.platform !== "darwin") {
  console.log("Skipping macOS Liquid Glass icon compile on non-darwin host");
  process.exit(0);
}

await cp(
  path.join(iconsDir, "app-icon.png"),
  path.join(iconComposerDir, "Assets", "OpenKara 2.png"),
);

await rm(stagingDir, { recursive: true, force: true });
await mkdir(stagingDir, { recursive: true });

execFileSync(
  "xcrun",
  [
    "actool",
    iconComposerDir,
    "--compile",
    stagingDir,
    "--app-icon",
    "OpenKara",
    "--platform",
    "macosx",
    "--minimum-deployment-target",
    "11.0",
    "--target-device",
    "mac",
    "--output-partial-info-plist",
    path.join(stagingDir, "partial.plist"),
    "--output-format",
    "human-readable-text",
  ],
  { stdio: "inherit" },
);

await cp(path.join(stagingDir, "Assets.car"), path.join(iconsDir, "Assets.car"));
await cp(
  path.join(stagingDir, "OpenKara.icns"),
  path.join(iconsDir, "OpenKara.icns"),
);

await rm(stagingDir, { recursive: true, force: true });

console.log(
  "Compiled macOS Liquid Glass assets: src-tauri/icons/Assets.car, OpenKara.icns",
);
