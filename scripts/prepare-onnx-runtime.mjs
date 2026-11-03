// Stage the ONNX Runtime for dev, CI, and release builds from the pinned
// openkara-models catalog snapshot (src-tauri/catalog/release-manifest.json)
// — the same contract fixture the application, scripts/resolve-model.mjs,
// and scripts/setup.sh consume. No runtime version, URL, or digest is
// hardcoded here: updating the runtime pin means updating the snapshot.
//
// The archive digest is verified before extraction and every staged file is
// verified against the catalog's per-file digests after extraction.

import { createHash } from "node:crypto";
import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import {
  basename,
  dirname,
  isAbsolute,
  join,
  relative,
  resolve,
  win32,
} from "node:path";
import os from "node:os";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const ROOT_DIR = fileURLToPath(new URL("..", import.meta.url));
const CATALOG_MANIFEST_PATH = join(
  ROOT_DIR,
  "src-tauri",
  "catalog",
  "release-manifest.json",
);
const STAGING_DIR = join(ROOT_DIR, "src-tauri", "generated", "onnxruntime");
const MANIFEST_PATH = join(STAGING_DIR, "manifest.json");

const LIBRARY_BY_PLATFORM_SUFFIX = {
  "apple-darwin": "libonnxruntime.dylib",
  "linux-gnu": "libonnxruntime.so",
  "windows-msvc": "onnxruntime.dll",
};

function parseArgs(argv) {
  const parsed = {};
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] === "--target") {
      parsed.target = argv[index + 1];
      index += 1;
    }
    if (argv[index] === "--provider") {
      parsed.provider = argv[index + 1];
      index += 1;
    }
  }
  return parsed;
}

function defaultTargetForHost() {
  if (process.platform === "darwin") {
    return process.arch === "arm64"
      ? "aarch64-apple-darwin"
      : "x86_64-apple-darwin";
  }
  if (process.platform === "linux") {
    return process.arch === "arm64"
      ? "aarch64-unknown-linux-gnu"
      : "x86_64-unknown-linux-gnu";
  }
  if (process.platform === "win32") {
    return "x86_64-pc-windows-msvc";
  }
  throw new Error(`unsupported host platform '${process.platform}'`);
}

function defaultProviderForTarget(targetTriple) {
  return targetTriple.endsWith("windows-msvc") ? "directml" : "cpu";
}

function libraryNameForTarget(targetTriple) {
  for (const [suffix, library] of Object.entries(LIBRARY_BY_PLATFORM_SUFFIX)) {
    if (targetTriple.endsWith(suffix)) {
      return library;
    }
  }
  throw new Error(`unsupported target '${targetTriple}'`);
}

function resolveCatalogRuntime(targetTriple, preferredProvider) {
  const catalog = JSON.parse(readFileSync(CATALOG_MANIFEST_PATH, "utf8"));
  if (
    !["openkara.catalog/release-v1", "openkara.catalog/release-v2"].includes(
      catalog.schema_version,
    )
  ) {
    throw new Error(
      `unsupported release manifest schema: ${catalog.schema_version}`,
    );
  }
  // Skip deprecated runtimes; when more than one active runtime matches the
  // target, disambiguate by the preferred execution provider.
  const matches = catalog.artifacts.runtimes.filter(
    (runtime) =>
      runtime.target_triple === targetTriple &&
      !runtime.deprecation?.deprecated,
  );
  if (matches.length === 0) {
    throw new Error(
      `catalog snapshot has no active runtime for target ${targetTriple}`,
    );
  }
  if (matches.length === 1) {
    return { catalog, runtime: matches[0] };
  }
  const advertises = (runtime, provider) =>
    runtime.runtime.execution_providers.some((ep) => ep === provider);
  let selected;
  if (preferredProvider === "directml") {
    selected =
      matches.find((runtime) => advertises(runtime, "directml")) ??
      matches.find(
        (runtime) =>
          runtime.runtime.execution_providers.length === 1 &&
          runtime.runtime.execution_providers[0] === "cpu",
      );
  } else {
    selected =
      matches.find(
        (runtime) =>
          runtime.runtime.execution_providers.length === 1 &&
          runtime.runtime.execution_providers[0] === "cpu",
      ) ?? matches.find((runtime) => !advertises(runtime, "directml"));
  }
  if (!selected) {
    throw new Error(
      `catalog snapshot lists ${matches.length} active runtimes for target ${targetTriple} and none matches preferred provider ${preferredProvider}`,
    );
  }
  return { catalog, runtime: selected };
}

function sha256Hex(buffer) {
  return createHash("sha256").update(buffer).digest("hex");
}

const TOOL_PROBES = {
  powershell: ["-NoProfile", "-NonInteractive", "-Command", "exit 0"],
  tar: ["--version"],
  unzip: ["-v"],
};

function ensureSystemTool(toolName) {
  try {
    execFileSync(toolName, TOOL_PROBES[toolName] ?? ["--version"], {
      stdio: "ignore",
    });
  } catch {
    throw new Error(`required tool '${toolName}' is not installed`);
  }
}

// .zip archives use Expand-Archive on Windows and unzip on macOS/Linux.
// .tar.gz uses tar, with --force-local on Windows so "C:\path" is not
// interpreted as host:path.
function isZipArchive(archivePath) {
  return archivePath.endsWith(".zip") || archivePath.endsWith(".nupkg");
}

function extractArchive(archivePath, destDir) {
  if (isZipArchive(archivePath)) {
    if (process.platform !== "win32") {
      execFileSync("unzip", ["-q", archivePath, "-d", destDir], {
        stdio: "inherit",
      });
      return;
    }

    const zipStub = archivePath.endsWith(".zip")
      ? archivePath
      : archivePath + ".zip";
    if (zipStub !== archivePath) {
      cpSync(archivePath, zipStub);
    }
    execFileSync(
      "powershell",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        `Expand-Archive -LiteralPath '${zipStub}' -DestinationPath '${destDir}' -Force`,
      ],
      { stdio: "inherit" },
    );
    if (zipStub !== archivePath) {
      rmSync(zipStub, { force: true });
    }
    return;
  }

  const tarArgs = process.platform === "win32" ? ["--force-local"] : [];
  execFileSync("tar", ["-xf", archivePath, "-C", destDir, ...tarArgs], {
    stdio: "inherit",
  });
}

function verifyArchiveLinks(destDir) {
  const root = realpathSync(destDir);
  const withinRoot = (path) => {
    const remainder = relative(root, path);
    return (
      remainder !== ".." &&
      !remainder.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`) &&
      !isAbsolute(remainder)
    );
  };
  const visit = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        visit(path);
      } else if (entry.isSymbolicLink()) {
        const target = readlinkSync(path);
        if (
          isAbsolute(target) ||
          win32.isAbsolute(target) ||
          target.includes("\\")
        ) {
          throw new Error(`archive link target must be relative: ${path}`);
        }
        const parent = resolve(root, relative(destDir, dirname(path)));
        if (
          !withinRoot(resolve(parent, target)) ||
          !withinRoot(realpathSync(path)) ||
          !statSync(path).isFile()
        ) {
          throw new Error(
            `archive link escapes or does not target a file: ${path}`,
          );
        }
      }
    }
  };
  visit(destDir);
}

const args = parseArgs(process.argv.slice(2));
const targetTriple =
  args.target ?? process.env.OPENKARA_ORT_TARGET ?? defaultTargetForHost();
const preferredProvider =
  args.provider ??
  process.env.OPENKARA_ORT_PROVIDER ??
  defaultProviderForTarget(targetTriple);
const libraryName = libraryNameForTarget(targetTriple);
const { runtime } = resolveCatalogRuntime(targetTriple, preferredProvider);

const declaredFiles = runtime.extracted_file_digests;
const declaredNames = Object.keys(declaredFiles);
const libraryCandidates = declaredNames.filter((name) => {
  const parts = name.split("/");
  if (parts.length === 1) return name === libraryName;
  if (parts.length !== 3 || parts[1] !== "lib") return false;
  const file = parts[2];
  return (
    file === libraryName ||
    (libraryName === "libonnxruntime.dylib" &&
      /^libonnxruntime\.\d.*\.dylib$/.test(file)) ||
    (libraryName === "libonnxruntime.so" &&
      /^libonnxruntime\.so\.\d/.test(file))
  );
});
const librarySource =
  libraryCandidates.find((name) => basename(name) === libraryName) ??
  libraryCandidates[0];
if (!librarySource) {
  throw new Error(
    `catalog runtime ${runtime.artifact_id} does not declare ${libraryName}`,
  );
}
const companionNames = declaredNames.filter((name) => {
  const parts = name.split("/");
  return (
    parts.length === 3 &&
    parts[1] === "lib" &&
    /^onnxruntime_providers_shared\.dll$|^libonnxruntime_providers_shared\.so$/.test(
      parts[2],
    )
  );
});
const stagedFileDigests = {
  ...declaredFiles,
  [libraryName]: declaredFiles[librarySource],
  ...Object.fromEntries(
    companionNames.map((name) => [basename(name), declaredFiles[name]]),
  ),
};
const stagedFileNames = Object.keys(stagedFileDigests);

// Fast path: everything already staged from this exact artifact.
const stagedRuntimePath = join(STAGING_DIR, libraryName);
if (existsSync(MANIFEST_PATH)) {
  const staged = JSON.parse(readFileSync(MANIFEST_PATH, "utf8"));
  if (
    staged.artifact_id === runtime.artifact_id &&
    Object.entries(stagedFileDigests).every(([name, digest]) => {
      const path = join(STAGING_DIR, name);
      if (!existsSync(path)) return false;
      const bytes = readFileSync(path);
      return bytes.length === digest.size && sha256Hex(bytes) === digest.sha256;
    })
  ) {
    console.log(`ONNX Runtime already prepared at ${stagedRuntimePath}`);
    process.exit(0);
  }
}

ensureSystemTool(
  isZipArchive(runtime.filename)
    ? process.platform === "win32"
      ? "powershell"
      : "unzip"
    : "tar",
);

const tempRoot = mkdtempSync(join(os.tmpdir(), "openkara-ort-"));
const archivePath = join(tempRoot, runtime.filename);
const extractedDir = join(tempRoot, "extracted");
mkdirSync(extractedDir, { recursive: true });

try {
  console.log(`Downloading ${runtime.download_url}`);
  const response = await fetch(runtime.download_url);
  if (!response.ok) {
    throw new Error(
      `failed to download ${runtime.download_url}: ${response.status} ${response.statusText}`,
    );
  }

  const archiveBytes = Buffer.from(await response.arrayBuffer());
  if (archiveBytes.length !== runtime.byte_size) {
    throw new Error(
      `archive size mismatch: expected ${runtime.byte_size} bytes, got ${archiveBytes.length}`,
    );
  }
  const actualDigest = sha256Hex(archiveBytes);
  if (actualDigest !== runtime.archive_digest) {
    throw new Error(
      `archive digest mismatch: expected ${runtime.archive_digest}, got ${actualDigest}`,
    );
  }

  writeFileSync(archivePath, archiveBytes);
  extractArchive(archivePath, extractedDir);
  verifyArchiveLinks(extractedDir);

  rmSync(STAGING_DIR, { force: true, recursive: true });
  mkdirSync(STAGING_DIR, { recursive: true });

  for (const [fileName, digest] of Object.entries(declaredFiles)) {
    const extractedPath = join(extractedDir, fileName);
    if (!existsSync(extractedPath)) {
      throw new Error(
        `archive is missing declared file ${fileName} (expected at its root)`,
      );
    }
    const bytes = readFileSync(extractedPath);
    if (bytes.length !== digest.size) {
      throw new Error(
        `extracted file ${fileName} has size ${bytes.length}, expected ${digest.size}`,
      );
    }
    const fileDigest = sha256Hex(bytes);
    if (fileDigest !== digest.sha256) {
      throw new Error(`extracted file ${fileName} digest mismatch`);
    }
    const stagedPath = join(STAGING_DIR, fileName);
    mkdirSync(dirname(stagedPath), { recursive: true });
    copyFileSync(extractedPath, stagedPath);
  }

  if (librarySource !== libraryName) {
    copyFileSync(join(extractedDir, librarySource), stagedRuntimePath);
  }
  for (const name of companionNames) {
    copyFileSync(join(extractedDir, name), join(STAGING_DIR, basename(name)));
  }

  writeFileSync(
    MANIFEST_PATH,
    JSON.stringify(
      {
        artifact_id: runtime.artifact_id,
        version: runtime.runtime.version,
        target: targetTriple,
        sourceArchive: runtime.filename,
        files: stagedFileNames,
      },
      null,
      2,
    ) + "\n",
  );

  console.log(`Prepared ONNX Runtime at ${stagedRuntimePath}`);
} finally {
  rmSync(tempRoot, { force: true, recursive: true });
}
