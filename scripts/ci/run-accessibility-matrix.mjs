import { spawnSync } from "node:child_process";

const command = process.platform === "win32" ? "pnpm.cmd" : "pnpm";
const result = spawnSync(command, ["test:a11y"], {
  stdio: "inherit",
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
