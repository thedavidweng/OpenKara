import {
  cpSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import os from "node:os";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const ORT_VERSION = "1.26.0";
// DO NOT upgrade this. ORT 1.24.1+ dropped x86_64 macOS prebuilt binaries
// (documented breaking change in v1.24.1 release notes). v1.23.2 is the
// last version that ships onnxruntime-osx-x86_64-*.tgz.
const ORT_VERSION_LEGACY_X86_64_MACOS = "1.23.2";
const WINDOWS_ORT_PACKAGE_NAME = "Microsoft.ML.OnnxRuntime.DirectML";
// Microsoft.ML.OnnxRuntime.DirectML lags behind the base package on NuGet.
// Pin to the latest available version (1.24.4) rather than ORT_VERSION.
const WINDOWS_ORT_PACKAGE_VERSION = "1.24.4";
const WINDOWS_DIRECTML_PACKAGE_NAME = "Microsoft.AI.DirectML";
const WINDOWS_DIRECTML_PACKAGE_VERSION = "1.15.4";
const ROOT_DIR = fileURLToPath(new URL("..", import.meta.url));
const STAGING_DIR = join(ROOT_DIR, "src-tauri", "generated", "onnxruntime");
const MANIFEST_PATH = join(STAGING_DIR, "manifest.json");

const TARGET_CONFIG = {
  "aarch64-apple-darwin": {
    archiveName: `onnxruntime-osx-arm64-${ORT_VERSION}.tgz`,
    outputName: "libonnxruntime.dylib",
    manifestTarget: "aarch64-apple-darwin",
    runtimeVersion: ORT_VERSION,
    sourceKind: "github-release",
  },
  "x86_64-apple-darwin": {
    archiveName: `onnxruntime-osx-x86_64-${ORT_VERSION_LEGACY_X86_64_MACOS}.tgz`,
    outputName: "libonnxruntime.dylib",
    manifestTarget: "x86_64-apple-darwin",
    runtimeVersion: ORT_VERSION_LEGACY_X86_64_MACOS,
    sourceKind: "github-release",
  },
  "x86_64-unknown-linux-gnu": {
    archiveName: `onnxruntime-linux-x64-${ORT_VERSION}.tgz`,
    outputName: "libonnxruntime.so",
    manifestTarget: "x86_64-unknown-linux-gnu",
    runtimeVersion: ORT_VERSION,
    sourceKind: "github-release",
  },
  "aarch64-unknown-linux-gnu": {
    archiveName: `onnxruntime-linux-aarch64-${ORT_VERSION}.tgz`,
    outputName: "libonnxruntime.so",
    manifestTarget: "aarch64-unknown-linux-gnu",
    runtimeVersion: ORT_VERSION,
    sourceKind: "github-release",
  },
  "x86_64-pc-windows-msvc": {
    archiveName: `${WINDOWS_ORT_PACKAGE_NAME}.${WINDOWS_ORT_PACKAGE_VERSION}.nupkg`,
    outputName: "onnxruntime.dll",
    companionFiles: ["onnxruntime_providers_shared.dll", "DirectML.dll"],
    manifestTarget: "x86_64-pc-windows-msvc",
    packageName: WINDOWS_ORT_PACKAGE_NAME,
    runtimeVersion: WINDOWS_ORT_PACKAGE_VERSION,
    dependencyPackages: [
      {
        archiveName: `${WINDOWS_DIRECTML_PACKAGE_NAME}.${WINDOWS_DIRECTML_PACKAGE_VERSION}.nupkg`,
        fileName: "DirectML.dll",
        packageName: WINDOWS_DIRECTML_PACKAGE_NAME,
        version: WINDOWS_DIRECTML_PACKAGE_VERSION,
      },
    ],
    sourceKind: "nuget-package",
  },
};

function runtimeMatcher(outputName) {
  if (outputName === "onnxruntime.dll") {
    return (fileName) => fileName === outputName;
  }

  if (outputName.endsWith(".dylib")) {
    return (fileName) =>
      fileName.startsWith("libonnxruntime") &&
      fileName.endsWith(".dylib") &&
      !fileName.includes("providers");
  }

  return (fileName) =>
    fileName.startsWith("libonnxruntime.so") &&
    !fileName.includes("providers") &&
    !fileName.endsWith(".a");
}

function parseArgs(argv) {
  const parsed = {};

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--target") {
      parsed.target = argv[index + 1];
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

function walkFiles(dirPath, files = []) {
  for (const entry of readdirSync(dirPath, { withFileTypes: true })) {
    const fullPath = join(dirPath, entry.name);
    if (entry.isDirectory()) {
      walkFiles(fullPath, files);
      continue;
    }

    if (entry.isFile() || entry.isSymbolicLink()) {
      files.push(fullPath);
    }
  }

  return files;
}

function readManifest() {
  if (!existsSync(MANIFEST_PATH)) {
    return null;
  }

  return JSON.parse(readFileSync(MANIFEST_PATH, "utf8"));
}

function ensureSystemTool(toolName) {
  try {
    execFileSync(toolName, ["--version"], { stdio: "ignore" });
  } catch {
    throw new Error(`required tool '${toolName}' is not installed`);
  }
}

function archiveUrlForPackage(packageName, version) {
  return `https://www.nuget.org/api/v2/package/${packageName}/${version}`;
}

function archiveUrlFor(config) {
  if (config.sourceKind === "nuget-package") {
    return archiveUrlForPackage(config.packageName, config.runtimeVersion);
  }

  return `https://github.com/microsoft/onnxruntime/releases/download/v${config.runtimeVersion}/${config.archiveName}`;
}

const args = parseArgs(process.argv.slice(2));
const targetTriple =
  args.target ?? process.env.OPENKARA_ORT_TARGET ?? defaultTargetForHost();
const config = TARGET_CONFIG[targetTriple];

if (!config) {
  throw new Error(`unsupported target '${targetTriple}'`);
}

const manifest = readManifest();
const stagedRuntimePath = join(STAGING_DIR, config.outputName);
const stagedCompanionPaths = (config.companionFiles ?? []).map((fileName) =>
  join(STAGING_DIR, fileName),
);
if (
  manifest?.version === config.runtimeVersion &&
  manifest?.target === config.manifestTarget &&
  existsSync(stagedRuntimePath) &&
  stagedCompanionPaths.every((filePath) => existsSync(filePath))
) {
  console.log(`ONNX Runtime already prepared at ${stagedRuntimePath}`);
  process.exit(0);
}

ensureSystemTool("tar");

// .nupkg (NuGet) files are ZIP archives, not tar. macOS bsdtar can extract
// ZIPs transparently, but GNU tar (Git Bash on Windows) cannot. Use
// .NET ZipFile on Windows (Expand-Archive rejects .nupkg extensions).
// For .tgz archives, GNU tar also needs --force-local so "C:\path" is not
// parsed as host:path.
function extractArchive(archivePath, destDir, isZip) {
  if (isZip && process.platform === "win32") {
    execFileSync(
      "powershell",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Add-Type -AssemblyName System.IO.Compression.FileSystem; " +
          `[System.IO.Compression.ZipFile]::ExtractToDirectory('${archivePath}', '${destDir}', $true)`,
      ],
      { stdio: "inherit" },
    );
    return;
  }

  const tarArgs = process.platform === "win32" ? ["--force-local"] : [];
  execFileSync("tar", ["-xf", archivePath, "-C", destDir, ...tarArgs], {
    stdio: "inherit",
  });
}

const isNugetPackage = config.sourceKind === "nuget-package";

const tempRoot = mkdtempSync(join(os.tmpdir(), "openkara-ort-"));
const archivePath = join(tempRoot, config.archiveName);
const extractedDir = join(tempRoot, "extracted");
mkdirSync(extractedDir, { recursive: true });

try {
  const archiveUrl = archiveUrlFor(config);
  console.log(`Downloading ${archiveUrl}`);

  const response = await fetch(archiveUrl);
  if (!response.ok) {
    throw new Error(
      `failed to download ${archiveUrl}: ${response.status} ${response.statusText}`,
    );
  }

  const archiveBytes = Buffer.from(await response.arrayBuffer());
  writeFileSync(archivePath, archiveBytes);
  extractArchive(archivePath, extractedDir, isNugetPackage);

  for (const dependencyPackage of config.dependencyPackages ?? []) {
    const dependencyUrl = archiveUrlForPackage(
      dependencyPackage.packageName,
      dependencyPackage.version,
    );
    const dependencyArchivePath = join(tempRoot, dependencyPackage.archiveName);

    console.log(`Downloading ${dependencyUrl}`);
    const dependencyResponse = await fetch(dependencyUrl);
    if (!dependencyResponse.ok) {
      throw new Error(
        `failed to download ${dependencyUrl}: ${dependencyResponse.status} ${dependencyResponse.statusText}`,
      );
    }

    writeFileSync(
      dependencyArchivePath,
      Buffer.from(await dependencyResponse.arrayBuffer()),
    );
    // Dependency packages from NuGet are also ZIP (.nupkg) archives.
    extractArchive(dependencyArchivePath, extractedDir, isNugetPackage);
  }

  const runtimeCandidate = walkFiles(extractedDir).find((filePath) =>
    runtimeMatcher(config.outputName)(filePath.split(/[\\/]/).pop() ?? ""),
  );
  if (!runtimeCandidate) {
    throw new Error(
      `failed to locate ${config.outputName} inside ${config.archiveName}`,
    );
  }

  rmSync(STAGING_DIR, { force: true, recursive: true });
  mkdirSync(STAGING_DIR, { recursive: true });
  cpSync(realpathSync(runtimeCandidate), stagedRuntimePath);

  for (const companionFile of config.companionFiles ?? []) {
    const companionCandidate = walkFiles(extractedDir).find(
      (filePath) => (filePath.split(/[\\/]/).pop() ?? "") === companionFile,
    );
    if (!companionCandidate) {
      throw new Error(
        `failed to locate ${companionFile} inside ${config.archiveName}`,
      );
    }

    cpSync(realpathSync(companionCandidate), join(STAGING_DIR, companionFile));
  }

  writeFileSync(
    MANIFEST_PATH,
    JSON.stringify(
      {
        version: config.runtimeVersion,
        target: config.manifestTarget,
        sourceArchive: config.archiveName,
        sourceKind: config.sourceKind,
        files: [config.outputName, ...(config.companionFiles ?? [])],
      },
      null,
      2,
    ) + "\n",
  );

  console.log(`Prepared ONNX Runtime at ${stagedRuntimePath}`);
} finally {
  rmSync(tempRoot, { force: true, recursive: true });
}
