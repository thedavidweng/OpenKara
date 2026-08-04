import { spawnSync } from "node:child_process";

// Windows runners need a shell to resolve pnpm.cmd; without shell: true the
// spawnSync call fails with EINVAL.
const result = spawnSync("pnpm", ["test:a11y"], {
  stdio: "inherit",
  shell: process.platform === "win32",
  env: {
    ...process.env,
    OKA_ACCESSIBILITY_MATRIX: "1",
  },
});

if (result.error) {
  throw result.error;
}
if (result.status === null) {
  throw new Error("Accessibility matrix process exited without a status code");
}
process.exitCode = result.status;
