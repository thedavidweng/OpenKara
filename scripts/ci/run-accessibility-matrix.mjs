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
process.exitCode = result.status;
