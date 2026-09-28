import { readFileSync, writeFileSync } from "node:fs";

const lock = JSON.parse(readFileSync("sidecars/netease/package-lock.json"));
const runtime = JSON.parse(readFileSync("sidecars/netease/runtime.json"));
const sources = Object.entries(runtime.targets)
  .filter(([target]) => target.endsWith("unknown-linux-gnu"))
  .map(([target, [suffix, sha256]]) => ({
    type: "file",
    url: `https://nodejs.org/dist/v${runtime.version}/node-v${runtime.version}-${suffix}`,
    sha256,
    dest: "flatpak-netease",
    "dest-filename": `node-v${runtime.version}-${suffix}`,
    "only-arches": [target.startsWith("aarch64") ? "aarch64" : "x86_64"],
  }));
for (const [path, entry] of Object.entries(lock.packages)) {
  if (!path) continue;
  sources.push({
    type: "file",
    url: entry.resolved,
    sha512: Buffer.from(
      entry.integrity.slice("sha512-".length),
      "base64",
    ).toString("hex"),
    dest: "flatpak-netease/tarballs",
    "dest-filename": `${path.replaceAll("/", "__")}.tgz`,
  });
}
writeFileSync(
  "packaging/flatpak/generated/netease-sources.0.json",
  `${JSON.stringify(sources, null, 2)}\n`,
);
