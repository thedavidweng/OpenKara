import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  accessSync,
  chmodSync,
  constants,
  createReadStream,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  statSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";

function option(name) {
  const index = process.argv.indexOf(`--${name}`);
  const value = index < 0 ? undefined : process.argv[index + 1];
  if (!value || value.startsWith("--")) throw new Error(`missing --${name}`);
  return value;
}

function requireObservation(condition, message) {
  if (!condition) throw new Error(message);
}

function optionalFile(file) {
  try {
    return existsSync(file) ? readFileSync(file, "utf8") : null;
  } catch (error) {
    return `unavailable: ${error.code}`;
  }
}

function commandResult(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    timeout: 300_000,
    maxBuffer: 16 * 1024 * 1024,
    ...options,
  });
  return {
    command: [command, ...args],
    status: result.status,
    signal: result.signal,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
    error: result.error?.message ?? null,
  };
}

function run(command, args, logFile, options) {
  const result = commandResult(command, args, options);
  writeFileSync(logFile, `${JSON.stringify(result, null, 2)}\n`);
  requireObservation(result.status === 0, `${command} failed; see ${logFile}`);
  return result.stdout.trim();
}

async function sha256(file) {
  const digest = createHash("sha256");
  for await (const chunk of createReadStream(file)) digest.update(chunk);
  return digest.digest("hex");
}

function environment() {
  let fuseDevice;
  try {
    const device = statSync("/dev/fuse");
    accessSync("/dev/fuse", constants.R_OK | constants.W_OK);
    fuseDevice = {
      present: true,
      character_device: device.isCharacterDevice(),
      readable_writable: true,
      mode: (device.mode & 0o7777).toString(8),
    };
  } catch (error) {
    fuseDevice = { present: existsSync("/dev/fuse"), error: error.message };
  }
  return {
    platform: process.platform,
    arch: process.arch,
    uid: process.getuid(),
    gid: process.getgid(),
    os_release: optionalFile("/etc/os-release"),
    kernel: commandResult("uname", ["-a"]),
    kernel_release: os.release(),
    fuse: {
      setup:
        "Install libfuse2 from the distribution; do not change device permissions, kernel settings, or security policy",
      device: fuseDevice,
      filesystems: optionalFile("/proc/filesystems"),
      packages: commandResult("dpkg-query", [
        "-W",
        "-f=${Package} ${Version} ${Status}\\n",
        "libfuse2",
        "fuse",
        "fuse3",
      ]),
    },
    security: {
      apparmor_enabled: optionalFile("/sys/module/apparmor/parameters/enabled"),
      process_attributes: optionalFile("/proc/self/attr/current"),
      process_status: optionalFile("/proc/self/status"),
      mount_namespace: commandResult("readlink", ["/proc/self/ns/mnt"]),
    },
  };
}

function appExecutable(root) {
  const matches = [];
  function visit(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) visit(file);
      else if (entry.name === "openkara" && statSync(file).isFile())
        matches.push(file);
    }
  }
  visit(root);
  requireObservation(
    matches.length === 1,
    `expected one bundled app, found ${matches.length}`,
  );
  return realpathSync(matches[0]);
}

async function appImage(artifact, output, report) {
  chmodSync(artifact, 0o755);
  const mount = spawn(artifact, ["--appimage-mount"], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  let failure = null;
  let finished = false;
  mount.stdout.on("data", (chunk) => {
    stdout += chunk;
  });
  mount.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  mount.on("error", (error) => {
    failure = error.message;
  });
  const closed = new Promise((resolve) => {
    mount.on("close", (status, signal) => {
      finished = true;
      resolve({ status, signal });
    });
  });
  let mountRoot;
  const deadline = Date.now() + 15_000;
  while (!finished && !failure && Date.now() < deadline) {
    mountRoot = stdout
      .split(/\r?\n/u)
      .find(
        (line) =>
          path.isAbsolute(line) && existsSync(path.join(line, "AppRun")),
      );
    if (mountRoot) break;
    await delay(100);
  }
  report.appimage = {
    mount_command: [artifact, "--appimage-mount"],
    mount_stdout: stdout,
    mount_stderr: stderr,
    mount_error: failure,
    mount_root: mountRoot ?? null,
    mountinfo: optionalFile("/proc/self/mountinfo"),
  };
  try {
    if (mountRoot) {
      const mountLine = report.appimage.mountinfo
        .split("\n")
        .find(
          (line) =>
            line
              .split(" ")[4]
              ?.replace(/\\([0-7]{3})/gu, (_, octal) =>
                String.fromCharCode(Number.parseInt(octal, 8)),
              ) === mountRoot,
        );
      requireObservation(
        mountLine?.includes(" - fuse"),
        "AppImage mount is not recorded as FUSE",
      );
      const executable = appExecutable(mountRoot);
      report.appimage.mode = "fuse-mount";
      report.appimage.decision =
        "FUSE mounted the distributed artifact; execute the original AppImage for each phase";
      report.appimage.mount_record = mountLine;
      report.appimage.bundled_executable = path.relative(mountRoot, executable);
      report.appimage.bundled_executable_sha256 = await sha256(executable);
      return {
        command: artifact,
        executable_sha256: report.appimage.bundled_executable_sha256,
        mode: "fuse-mount",
      };
    }
  } finally {
    if (!finished) mount.kill("SIGTERM");
    const result = await Promise.race([closed, delay(5_000).then(() => null)]);
    if (!result) {
      mount.kill("SIGKILL");
      await closed;
    }
    report.appimage.mount_result = result;
    report.appimage.mount_stdout = stdout;
    report.appimage.mount_stderr = stderr;
  }
  report.appimage.mode = "extraction-fallback";
  report.appimage.decision =
    "FUSE mount probe failed or timed out; execute AppRun from this artifact's extracted filesystem. This is not mounted-AppImage evidence";
  const extraction = path.join(output, "appimage-extracted");
  mkdirSync(extraction);
  run(
    artifact,
    ["--appimage-extract"],
    path.join(output, "appimage-extract.log"),
    { cwd: extraction },
  );
  const root = path.join(extraction, "squashfs-root");
  const executable = appExecutable(root);
  report.appimage.bundled_executable = path.relative(root, executable);
  report.appimage.bundled_executable_sha256 = await sha256(executable);
  return {
    command: path.join(root, "AppRun"),
    executable: realpathSync(executable),
    executable_sha256: report.appimage.bundled_executable_sha256,
    mode: "extraction-fallback",
  };
}

async function debianPackage(artifact, output, report) {
  const metadata = run(
    "dpkg-deb",
    ["--field", artifact],
    path.join(output, "deb-metadata.log"),
  );
  const packageName = metadata.match(/^Package: (.+)$/mu)?.[1];
  requireObservation(packageName, "Debian artifact has no package name");
  report.deb = { metadata, package: packageName };
  const payload = path.join(output, "deb-payload");
  run(
    "dpkg-deb",
    ["--extract", artifact, payload],
    path.join(output, "deb-extract.log"),
  );
  const payloadExecutable = appExecutable(payload);
  report.deb.payload_executable_sha256 = await sha256(payloadExecutable);
  run(
    "sudo",
    ["apt-get", "install", "--reinstall", "-y", artifact],
    path.join(output, "deb-install.log"),
  );
  report.deb.installed_package = run(
    "dpkg-query",
    [
      "-W",
      "-f=${Package} ${Version} ${Architecture} ${Status}\\n",
      packageName,
    ],
    path.join(output, "deb-installed.log"),
  );
  requireObservation(
    report.deb.installed_package.includes("install ok installed"),
    "Debian package was not installed",
  );
  const files = run(
    "dpkg-query",
    ["-L", packageName],
    path.join(output, "deb-files.log"),
  );
  const executables = files
    .split("\n")
    .filter((file) => file.endsWith("/bin/openkara"));
  requireObservation(
    executables.length === 1,
    "expected one installed Debian app executable",
  );
  const executable = realpathSync(executables[0]);
  requireObservation(
    (await sha256(executable)) === report.deb.payload_executable_sha256,
    "installed executable differs from the supplied Debian artifact payload",
  );
  report.deb.executable_owner = run(
    "dpkg-query",
    ["-S", executables[0]],
    path.join(output, "deb-executable-owner.log"),
  );
  return {
    command: executable,
    executable,
    executable_sha256: report.deb.payload_executable_sha256,
    mode: "deb-installed",
  };
}

async function observe(launch, output, report) {
  const root = path.join(output, "app-data");
  const pin = JSON.parse(
    readFileSync(
      new URL("../../packaging/source-runtime.json", import.meta.url),
      "utf8",
    ),
  );
  const phases = [];
  for (const phase of ["install", "restart"]) {
    const command = ["--automation-streaming-runtime", phase, root];
    run(launch.command, command, path.join(output, `${phase}.log`), {
      cwd: path.dirname(launch.command),
    });
    const observation = JSON.parse(
      readFileSync(path.join(root, `streaming-runtime-${phase}.json`), "utf8"),
    );
    const targetPin = pin.targets[observation.target];
    requireObservation(
      observation.phase === phase,
      "app reported the wrong phase",
    );
    requireObservation(
      targetPin?.archive_suffix === `linux-${process.arch}.tar.gz`,
      "app reported the wrong Linux target",
    );
    requireObservation(
      observation.archive_sha256 === targetPin.archive_sha256,
      "archive digest differs from the committed runtime pin",
    );
    requireObservation(
      observation.archive_url ===
        `https://nodejs.org/dist/v${pin.version}/node-v${pin.version}-${targetPin.archive_suffix}`,
      "runtime download URL differs from the pin",
    );
    requireObservation(
      observation.runtime.version === `v${pin.version}`,
      "downloaded Node version differs from the pin",
    );
    requireObservation(
      observation.runtime.platform === "linux" &&
        observation.runtime.arch === process.arch,
      "Node reported the wrong platform or architecture",
    );
    requireObservation(
      observation.app_pid > 0 &&
        observation.runtime.parent_pid === observation.app_pid,
      "Node is not the distributed app's direct child",
    );
    requireObservation(
      observation.app_sha256 === launch.executable_sha256,
      "running app hash differs from the distributed executable",
    );
    if (launch.executable) {
      requireObservation(
        observation.app_executable === launch.executable,
        "app did not run from the installed artifact",
      );
    } else {
      requireObservation(
        observation.app_executable.includes("/.mount_") &&
          observation.app_executable.endsWith(
            `/${report.appimage.bundled_executable}`,
          ),
        "app did not run from an AppImage mount",
      );
    }
    requireObservation(
      observation.runtime_executable.startsWith(`${realpathSync(root)}/`),
      "runtime is outside the isolated app-data directory",
    );
    requireObservation(
      (await sha256(observation.runtime_executable)) ===
        observation.runtime_sha256,
      "observed runtime hash differs from the installed file",
    );
    phases.push(observation);
    report.phases = phases;
  }
  requireObservation(
    phases[0].runtime_executable === phases[1].runtime_executable &&
      phases[0].runtime_sha256 === phases[1].runtime_sha256,
    "restart did not reuse the installed runtime",
  );
  report.invariants = {
    pinned_node_version_platform_arch: true,
    node_parent_is_distributed_app_each_phase: true,
    app_matches_distributed_executable_each_phase: true,
    restart_reuses_runtime_path_and_sha256: true,
  };
}

async function main() {
  const format = option("format");
  requireObservation(
    format === "appimage" || format === "deb",
    "expected --format appimage or deb",
  );
  const artifactDir = path.resolve(option("artifact-dir"));
  const output = path.resolve(option("output"));
  mkdirSync(output, { recursive: true });
  const report = {
    schema_version: 1,
    status: "failed",
    observed_at: new Date().toISOString(),
    format,
    commit_sha: process.env.GITHUB_SHA ?? null,
    workflow_run_id: process.env.GITHUB_RUN_ID ?? null,
    environment: environment(),
  };
  try {
    requireObservation(
      process.platform === "linux",
      "run this observer on the target Linux system",
    );
    const suffix = format === "appimage" ? ".AppImage" : ".deb";
    const artifacts = readdirSync(artifactDir).filter((name) =>
      name.endsWith(suffix),
    );
    requireObservation(
      artifacts.length === 1,
      `expected one ${suffix} artifact`,
    );
    const artifact = path.join(artifactDir, artifacts[0]);
    report.artifact = {
      path: artifact,
      bytes: statSync(artifact).size,
      sha256: await sha256(artifact),
    };
    const launch =
      format === "appimage"
        ? await appImage(artifact, output, report)
        : await debianPackage(artifact, output, report);
    launch.executable_sha256 ??= await sha256(launch.executable);
    report.launch = launch;
    await observe(launch, output, report);
    report.status = "passed";
  } catch (error) {
    report.error = error.message;
    throw error;
  } finally {
    report.completed_at = new Date().toISOString();
    writeFileSync(
      path.join(output, "linux-streaming-runtime-observation.json"),
      `${JSON.stringify(report, null, 2)}\n`,
    );
    if (process.env.GITHUB_STEP_SUMMARY) {
      const summary = `## Linux ${format} streaming runtime\n\nResult: ${report.status}\n\nMode: ${report.launch?.mode ?? report.appimage?.mode ?? "not launched"}\n\nArtifact SHA-256: ${report.artifact?.sha256 ?? "not available"}\n\n${report.error ?? "Pinned Node install, direct app parent, and restart identity passed."}\n`;
      writeFileSync(process.env.GITHUB_STEP_SUMMARY, summary, { flag: "a" });
    }
  }
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
