import { execFileSync } from "node:child_process";
import { readdir, rm } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const projectRoot = path.resolve(__dirname, "..");
const targetRoot = path.join(projectRoot, "src-tauri", "target");

if (process.platform !== "darwin") {
  console.log("Skipping macOS bundle cleanup on non-darwin host");
  process.exit(0);
}

/**
 * RATIONALE: A failed/interrupted `bundle_dmg.sh` leaves a read/write
 * interstitial DMG attached under /Volumes/dmg.* and a `rw.<pid>.*.dmg`
 * next to the .app. The next `pnpm tauri build` then fails with a opaque
 * "failed to run bundle_dmg.sh" because hdiutil cannot recreate/attach
 * while that leftover is still mounted. Detach before deleting.
 */
function detachLeftoverBundleMounts() {
  let info;
  try {
    info = execFileSync("hdiutil", ["info"], { encoding: "utf8" });
  } catch {
    return [];
  }

  const detached = [];
  const blocks = info.split(/^={10,}$/m);
  for (const block of blocks) {
    if (!block.includes(`${path.sep}bundle${path.sep}`)) {
      continue;
    }
    if (
      !block.includes("OpenKara") &&
      !/rw\.\d+\./.test(block) &&
      !/\/Volumes\/dmg\./.test(block)
    ) {
      continue;
    }

    const mountMatch = block.match(/(\/Volumes\/[^\s]+)/);
    if (!mountMatch) {
      continue;
    }
    const mountPoint = mountMatch[1];
    try {
      execFileSync("hdiutil", ["detach", mountPoint, "-force"], {
        stdio: "ignore",
      });
      detached.push(mountPoint);
    } catch {
      // Best-effort: a busy mount will still surface on the next delete/create.
    }
  }

  return detached;
}

async function collectBundleDirectories() {
  const directories = new Set([
    path.join(targetRoot, "release", "bundle", "macos"),
    path.join(targetRoot, "release", "bundle", "dmg"),
  ]);

  for (const entry of await readdir(targetRoot, { withFileTypes: true })) {
    if (!entry.isDirectory() || !entry.name.endsWith("-apple-darwin")) {
      continue;
    }

    directories.add(
      path.join(targetRoot, entry.name, "release", "bundle", "macos"),
    );
    directories.add(
      path.join(targetRoot, entry.name, "release", "bundle", "dmg"),
    );
  }

  return [...directories];
}

async function removeStrayDmgs(bundleDirectory) {
  let entries;
  try {
    entries = await readdir(bundleDirectory, { withFileTypes: true });
  } catch (error) {
    if (
      error &&
      typeof error === "object" &&
      "code" in error &&
      error.code === "ENOENT"
    ) {
      return [];
    }
    throw error;
  }

  const removed = [];
  for (const entry of entries) {
    // Final packaged DMGs live under bundle/dmg and must stay; only wipe
    // interstitial create-dmg leftovers (rw.<pid>.*) and stray copies under
    // the macos/ app directory.
    const isInterstitial = /^rw\.\d+\./.test(entry.name);
    const underMacos = bundleDirectory.endsWith(`${path.sep}macos`);
    if (!entry.isFile() || !entry.name.endsWith(".dmg")) {
      continue;
    }
    if (!isInterstitial && !underMacos) {
      continue;
    }

    const dmgPath = path.join(bundleDirectory, entry.name);
    await rm(dmgPath, { force: true });
    removed.push(dmgPath);
  }

  return removed;
}

const detached = detachLeftoverBundleMounts();
if (detached.length > 0) {
  console.log(`Detached ${detached.length} leftover DMG mount(s):`);
  for (const mountPoint of detached) {
    console.log(`- ${mountPoint}`);
  }
}

const removed = [];
for (const bundleDirectory of await collectBundleDirectories()) {
  removed.push(...(await removeStrayDmgs(bundleDirectory)));
}

if (removed.length === 0) {
  console.log("No stray macOS bundle DMGs found");
} else {
  console.log(`Removed ${removed.length} stray macOS bundle DMG(s):`);
  for (const dmgPath of removed) {
    console.log(`- ${path.relative(projectRoot, dmgPath)}`);
  }
}
