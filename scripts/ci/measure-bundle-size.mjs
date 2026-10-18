import { readdir, stat, writeFile } from "node:fs/promises";
import path from "node:path";

function option(values, name) {
  const index = values.indexOf(`--${name}`);
  const value = index >= 0 ? values[index + 1] : undefined;
  if (!value || value.startsWith("--")) {
    throw new Error(`missing --${name}`);
  }
  return value;
}

async function filesUnder(root, excludedPaths = []) {
  const entries = [];
  async function visit(current) {
    let children;
    try {
      children = await readdir(current, { withFileTypes: true });
    } catch (error) {
      if (error.code === "ENOENT") return;
      throw error;
    }
    for (const child of children) {
      const childPath = path.join(current, child.name);
      const relativePath = path.relative(root, childPath);
      if (
        excludedPaths.some(
          (excludedPath) =>
            relativePath === excludedPath ||
            relativePath.startsWith(`${excludedPath}${path.sep}`),
        )
      ) {
        continue;
      }
      if (child.isDirectory()) {
        await visit(childPath);
      } else if (child.isFile()) {
        const details = await stat(childPath);
        entries.push({ path: relativePath, byte_size: details.size });
      }
    }
  }
  await visit(root);
  return entries.sort((left, right) => left.path.localeCompare(right.path));
}

function summarize(name, root, files) {
  return {
    name,
    path: root,
    file_count: files.length,
    byte_size: files.reduce((total, file) => total + file.byte_size, 0),
  };
}

async function measureComponent(name, root, excludedPaths = []) {
  const rootStat = await stat(root).catch((error) => {
    if (error.code === "ENOENT") return null;
    throw error;
  });
  if (!rootStat) return null;

  const files = rootStat.isDirectory()
    ? await filesUnder(root, excludedPaths)
    : [{ path: path.basename(root), byte_size: rootStat.size }];
  return summarize(name, path.relative(process.cwd(), root), files);
}

async function main() {
  const values = process.argv.slice(2);
  const platform = option(values, "platform");
  const output = path.resolve(option(values, "output"));
  const bundleRoot = path.resolve(option(values, "bundle-root"));
  const binary = path.resolve(option(values, "binary"));
  const frontendRoot = path.resolve(option(values, "frontend-root"));
  const frontendDictRootValue = values.includes("--frontend-dict-root")
    ? option(values, "frontend-dict-root")
    : undefined;
  const frontendDictRoot = frontendDictRootValue
    ? path.resolve(frontendDictRootValue)
    : undefined;
  const runtimeRoot = path.resolve(option(values, "runtime-root"));

  const components = [];
  const frontendDictRelativePath = frontendDictRoot
    ? path.relative(frontendRoot, frontendDictRoot)
    : undefined;
  const frontend = await measureComponent(
    "frontend_dist",
    frontendRoot,
    frontendDictRelativePath && !frontendDictRelativePath.startsWith("..")
      ? [frontendDictRelativePath]
      : [],
  );
  if (frontend) components.push(frontend);
  if (frontendDictRoot) {
    const frontendDict = await measureComponent(
      "frontend_dict",
      frontendDictRoot,
    );
    if (frontendDict) components.push(frontendDict);
  }
  for (const [name, root] of [
    ["rust_binary", binary],
    ["onnx_runtime_input", runtimeRoot],
    ["bundle_output", bundleRoot],
  ]) {
    const component = await measureComponent(name, root);
    if (component) components.push(component);
  }

  const bundleFiles = await filesUnder(bundleRoot);
  const installerFiles = bundleFiles.filter(({ path: filePath }) =>
    /\.(?:AppImage|deb|dmg|exe|tar\.gz)$/u.test(filePath),
  );
  const report = {
    schema_version: 1,
    status: "measured",
    platform,
    measured_at: new Date().toISOString(),
    components,
    installers: installerFiles,
    installer_byte_size: installerFiles.reduce(
      (total, file) => total + file.byte_size,
      0,
    ),
  };
  await writeFile(output, `${JSON.stringify(report, null, 2)}\n`);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
