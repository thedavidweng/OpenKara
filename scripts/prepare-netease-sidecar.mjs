import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
  chmodSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const source = join(root, "sidecars/netease");
const pin = JSON.parse(readFileSync(join(source, "runtime.json"), "utf8"));
const host = `${process.arch === "arm64" ? "aarch64" : "x86_64"}-${{ darwin: "apple-darwin", linux: "unknown-linux-gnu", win32: "pc-windows-msvc" }[process.platform]}`;
const target =
  process.argv[2] === "--target"
    ? process.argv[3]
    : process.env.TAURI_TARGET || process.env.TAURI_ENV_TARGET_TRIPLE || host;
if (!pin.targets[target])
  throw new Error(`Unsupported NetEase sidecar target: ${target}`);
const [suffix, checksum] = pin.targets[target];
const output = join(root, "src-tauri/generated/netease");
const bin = join(root, "src-tauri/generated/bin");
const windows = target.endsWith("windows-msvc");
const executable = join(bin, `netease-node-${target}${windows ? ".exe" : ""}`);
const marker = join(output, "build-id");
const buildId = `${target}:${pin.version}:${checksum}:${createHash("sha256")
  .update(readFileSync(join(source, "package-lock.json")))
  .digest("hex")}`;
if (
  existsSync(executable) &&
  existsSync(
    join(output, "node_modules/@neteasecloudmusicapienhanced/api/package.json"),
  ) &&
  existsSync(marker) &&
  readFileSync(marker, "utf8") === buildId
) {
  for (const file of ["worker.mjs", "protocol.mjs"])
    cpSync(join(source, file), join(output, file));
  process.exit(0);
}
const archiveName = `node-v${pin.version}-${suffix}`;
const offlineRoot = process.env.OPENKARA_NETEASE_FLATPAK
  ? join(root, "flatpak-netease")
  : null;
const archive = offlineRoot
  ? readFileSync(join(offlineRoot, archiveName))
  : Buffer.from(
      await (async () => {
        const response = await fetch(
          `https://nodejs.org/dist/v${pin.version}/${archiveName}`,
        );
        if (!response.ok)
          throw new Error(`Node download failed: ${response.status}`);
        return response.arrayBuffer();
      })(),
    );
if (createHash("sha256").update(archive).digest("hex") !== checksum)
  throw new Error("Node archive checksum mismatch");
const temp = mkdtempSync(join(tmpdir(), "openkara-node-"));
try {
  const archivePath = join(temp, archiveName);
  writeFileSync(archivePath, archive);
  execFileSync("tar", ["-xf", archivePath, "-C", temp]);
  const extracted = join(temp, archiveName.replace(/\.(tar\.gz|zip)$/, ""));
  mkdirSync(bin, { recursive: true });
  cpSync(join(extracted, windows ? "node.exe" : "bin/node"), executable);
  if (!windows) chmodSync(executable, 0o755);
  rmSync(output, { recursive: true, force: true });
  mkdirSync(output, { recursive: true });
  for (const file of [
    "package.json",
    "package-lock.json",
    "worker.mjs",
    "protocol.mjs",
    "runtime.json",
  ])
    cpSync(join(source, file), join(output, file));
  cpSync(join(extracted, "LICENSE"), join(output, "NODE-LICENSE"));
  if (offlineRoot) {
    const lock = JSON.parse(readFileSync(join(output, "package-lock.json")));
    for (const [path, entry] of Object.entries(lock.packages)) {
      if (!path) continue;
      entry.resolved = `file:${join(offlineRoot, "tarballs", `${path.replaceAll("/", "__")}.tgz`)}`;
    }
    writeFileSync(join(output, "package-lock.json"), JSON.stringify(lock));
  }
  execFileSync(
    process.platform === "win32" ? "npm.cmd" : "npm",
    [
      "ci",
      "--omit=dev",
      "--ignore-scripts",
      "--no-audit",
      "--no-fund",
      ...(offlineRoot ? ["--offline"] : []),
    ],
    { cwd: output, stdio: "inherit", shell: process.platform === "win32" },
  );
  for (const packagePath of [
    "@neteasecloudmusicapienhanced/unblockmusic-utils",
    "@unblockneteasemusic/server",
  ])
    rmSync(join(output, "node_modules", packagePath), { recursive: true });
  rmSync(join(output, "node_modules/.bin"), { recursive: true });
  writeFileSync(marker, buildId);
  console.log(`Prepared NetEase sidecar for ${target}`);
} finally {
  rmSync(temp, { recursive: true, force: true });
}
