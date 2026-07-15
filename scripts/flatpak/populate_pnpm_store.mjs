/**
 * Populate a pnpm 11 content-addressable store from offline tarballs so
 * `pnpm install --offline` can resolve packages without registry access.
 *
 * pnpm 11 indexes packages in store-dir/v11/index.db (SQLite + msgpackr), not
 * the legacy per-package JSON files under index/. Writing only CAFS files (or
 * JSON indexes) leaves every package invisible and fails with
 * ERR_PNPM_NO_OFFLINE_TARBALL. We reuse pnpm's own extract worker so the
 * on-disk layout matches a normal `pnpm fetch`/`pnpm install` store.
 *
 * Requires: Node 22+ (node:sqlite, worker_threads) and an installed pnpm that
 * ships dist/worker.js (run after `npm install -g ./pnpm-package`).
 */
import { Worker } from "node:worker_threads";
import { DatabaseSync } from "node:sqlite";
import fs from "node:fs";
import path from "node:path";

function usage() {
  console.error(
    "Usage: node populate_pnpm_store.mjs <manifest.json> <tarball-dir> <store-dir> [pnpm-worker.js]",
  );
  process.exit(1);
}

function resolveWorkerPath(explicit) {
  if (explicit) {
    if (!fs.existsSync(explicit)) {
      throw new Error(`pnpm worker not found: ${explicit}`);
    }
    return path.resolve(explicit);
  }

  // Prefer the pnpm that is on PATH (Flatpak: .../pnpm-install/bin/pnpm).
  try {
    const which = process.env.PATH?.split(path.delimiter) ?? [];
    for (const dir of which) {
      for (const name of ["pnpm", "pnpm.cjs", "pnpm.mjs"]) {
        const candidate = path.join(dir, name);
        if (!fs.existsSync(candidate)) continue;
        let real = fs.realpathSync(candidate);
        // bin/pnpm -> ../lib/node_modules/pnpm/bin/pnpm.mjs (or package root)
        // Walk up looking for dist/worker.js
        for (let i = 0; i < 6; i++) {
          const worker = path.join(real, "dist", "worker.js");
          if (fs.existsSync(worker)) return worker;
          const parentWorker = path.join(
            path.dirname(real),
            "dist",
            "worker.js",
          );
          if (fs.existsSync(parentWorker)) return parentWorker;
          // lib/node_modules/pnpm/bin -> package root
          const pkgRoot = path.resolve(path.dirname(real), "..");
          const pkgWorker = path.join(pkgRoot, "dist", "worker.js");
          if (fs.existsSync(pkgWorker)) return pkgWorker;
          real = path.dirname(real);
        }
      }
    }
  } catch {
    // fall through
  }

  // Common Flatpak layout after npm install -g --prefix .../pnpm-install
  const flatpakWorker = path.resolve(
    "pnpm-install/lib/node_modules/pnpm/dist/worker.js",
  );
  if (fs.existsSync(flatpakWorker)) return flatpakWorker;

  const absoluteFlatpakWorker =
    "/run/build/openkara/pnpm-install/lib/node_modules/pnpm/dist/worker.js";
  if (fs.existsSync(absoluteFlatpakWorker)) return absoluteFlatpakWorker;

  throw new Error(
    "Could not locate pnpm dist/worker.js. Pass it as the 4th argument or put pnpm on PATH.",
  );
}

function integrityFromHex(integrityHex) {
  return `sha512-${Buffer.from(integrityHex, "hex").toString("base64")}`;
}

function storeIndexKey(integrity, pkgId) {
  return `${integrity}\t${pkgId}`;
}

function createWorkerPool(workerPath) {
  const worker = new Worker(workerPath);
  let busy = Promise.resolve();

  function run(message) {
    const task = busy.then(
      () =>
        new Promise((resolve, reject) => {
          const onMessage = (msg) => {
            cleanup();
            resolve(msg);
          };
          const onError = (err) => {
            cleanup();
            reject(err);
          };
          const cleanup = () => {
            worker.off("message", onMessage);
            worker.off("error", onError);
          };
          worker.on("message", onMessage);
          worker.on("error", onError);
          worker.postMessage(message);
        }),
    );
    // Keep the chain even if a task fails so later packages still run sequentially.
    busy = task.then(
      () => undefined,
      () => undefined,
    );
    return task;
  }

  async function close() {
    await busy;
    await worker.terminate();
  }

  return { run, close };
}

async function populateStore(manifestPath, tarballDir, storeDir, workerPath) {
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  const storeVersion = manifest.store_version;
  if (!storeVersion) {
    throw new Error("manifest missing store_version");
  }
  const packages = manifest.packages;
  if (!packages || typeof packages !== "object") {
    throw new Error("manifest missing packages");
  }

  const versionedStore = path.join(storeDir, storeVersion);
  fs.mkdirSync(path.join(versionedStore, "files"), { recursive: true });

  const dbPath = path.join(versionedStore, "index.db");
  // Start from a clean index so re-runs do not leave stale keys.
  if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);

  const db = new DatabaseSync(dbPath);
  db.exec(`
    CREATE TABLE IF NOT EXISTS package_index (
      key TEXT PRIMARY KEY,
      data BLOB NOT NULL
    ) WITHOUT ROWID;
  `);
  const insert = db.prepare(
    "INSERT OR REPLACE INTO package_index (key, data) VALUES (?, ?)",
  );

  const pool = createWorkerPool(workerPath);
  const entries = Object.entries(packages);
  let done = 0;

  try {
    for (const [tarballName, info] of entries) {
      const tarballPath = path.join(tarballDir, tarballName);
      if (!fs.existsSync(tarballPath)) {
        throw new Error(`Missing tarball: ${tarballPath}`);
      }
      const buffer = fs.readFileSync(tarballPath);
      const integrity = integrityFromHex(info.integrity_hex);
      const pkgId = `${info.name}@${info.version}`;
      const filesIndexFile = storeIndexKey(integrity, pkgId);

      const result = await pool.run({
        type: "extract",
        buffer,
        storeDir: versionedStore,
        integrity,
        filesIndexFile,
        readManifest: true,
        pkg: { name: info.name, version: info.version },
      });

      if (result.status !== "success") {
        const errMsg =
          result.error?.message ||
          result.error?.code ||
          JSON.stringify(result.error ?? result);
        throw new Error(
          `Failed to extract ${info.name}@${info.version}: ${errMsg}`,
        );
      }

      for (const write of result.indexWrites ?? []) {
        insert.run(write.key, Buffer.from(write.buffer));
      }

      done += 1;
      if (done % 50 === 0 || done === entries.length) {
        console.log(`populated pnpm store ${done}/${entries.length}`);
      }
    }
  } finally {
    await pool.close();
    db.close();
  }
}

const args = process.argv.slice(2);
if (args.length < 3 || args.length > 4) usage();

const [manifestPath, tarballDir, storeDir, workerArg] = args;
const workerPath = resolveWorkerPath(workerArg);

console.log(`using pnpm worker: ${workerPath}`);
await populateStore(manifestPath, tarballDir, storeDir, workerPath);
console.log("pnpm offline store population complete");
