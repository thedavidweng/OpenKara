import { spawnSync } from "node:child_process";
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { describe, expect, test } from "vitest";

function runGate(options: Record<string, string> = {}) {
  const root = mkdtempSync(join(tmpdir(), "openkara-signing-"));
  try {
    const bundle = join(root, "bundle");
    mkdirSync(join(bundle, "macos", "OpenKara.app"), { recursive: true });
    mkdirSync(join(bundle, "dmg"));
    writeFileSync(
      join(bundle, "macos", "OpenKara.app", "payload"),
      "signed app",
    );
    writeFileSync(join(bundle, "dmg", "OpenKara.dmg"), "disk image");
    const tar = spawnSync("tar", [
      "-czf",
      join(bundle, "macos", "OpenKara.app.tar.gz"),
      "-C",
      join(bundle, "macos"),
      "OpenKara.app",
    ]);
    expect(tar.status).toBe(0);
    writeFileSync(
      join(bundle, "macos", "OpenKara.app.tar.gz.sig"),
      "updater signature",
    );
    if (options.MISMATCH === "true") {
      writeFileSync(
        join(bundle, "macos", "OpenKara.app", "payload"),
        "different app",
      );
    }
    for (const tool of ["codesign", "xcrun", "spctl"]) {
      writeFileSync(
        join(root, tool),
        `#!/bin/bash
printf '%s\\n' "$(basename "$0") $*" >> "$CALL_LOG"
case "$(basename "$0") $1" in
  "codesign --verify") exit "$VERIFY_EXIT" ;;
  "codesign --display")
    printf '%s\\n' "Authority=$AUTHORITY" "TeamIdentifier=$TEAM" 'Timestamp=Oct 16, 2026' 'CodeDirectory v=20500 size=123 flags=0x10000(runtime)' >&2 ;;
  "xcrun notarytool") printf '{"id":"submission-id","status":"%s","message":"Processing complete"}\\n' "$NOTARY_STATUS" ;;
  "xcrun stapler") exit "$STAPLE_EXIT" ;;
  "spctl --assess") exit "$ASSESS_EXIT" ;;
esac
`,
        { mode: 0o755 },
      );
    }
    const result = spawnSync(
      "bash",
      [resolve("scripts/ci/notarize-macos-release.sh"), bundle],
      {
        encoding: "utf8",
        env: {
          ...process.env,
          PATH: `${root}:${process.env.PATH}`,
          CALL_LOG: join(root, "calls"),
          VERIFY_EXIT: "0",
          STAPLE_EXIT: "0",
          ASSESS_EXIT: "0",
          TEAM: "TESTTEAM",
          AUTHORITY: "Developer ID Application: Test (TESTTEAM)",
          NOTARY_STATUS: "Accepted",
          APPLE_TEAM_ID: "TESTTEAM",
          APPLE_API_KEY_PATH: join(root, "AuthKey_TEST.p8"),
          APPLE_API_KEY: "TESTKEY",
          APPLE_API_ISSUER: "test-issuer",
          ...options,
        },
      },
    );
    const calls = existsSync(join(root, "calls"))
      ? readFileSync(join(root, "calls"), "utf8")
      : "";
    return { ...result, calls };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

describe("macOS release gate", () => {
  test("verifies the app and updater before notarizing and stapling the DMG", () => {
    const result = runGate();
    expect(result.status, result.stderr).toBe(0);
    expect(result.calls.match(/codesign --verify/g)).toHaveLength(3);
    expect(result.calls.indexOf("--type execute")).toBeLessThan(
      result.calls.indexOf("notarytool submit"),
    );
    expect(result.calls.indexOf("notarytool submit")).toBeLessThan(
      result.calls.indexOf("stapler staple"),
    );
    expect(result.calls).toContain(
      "--type open --context context:primary-signature",
    );
    expect(result.calls).not.toContain("--force");
  });

  test.each([
    { TEAM: "WRONGTEAM" },
    { AUTHORITY: "Apple Distribution: Test (TESTTEAM)" },
    { VERIFY_EXIT: "1" },
    { STAPLE_EXIT: "1" },
    { ASSESS_EXIT: "1" },
    { MISMATCH: "true" },
    { APPLE_API_KEY: "" },
  ])("rejects invalid app or credentials before submission: %j", (options) => {
    const result = runGate(options);
    expect(result.status).not.toBe(0);
    expect(result.calls).not.toContain("notarytool submit");
  });

  test("does not staple a rejected DMG", () => {
    const result = runGate({ NOTARY_STATUS: "Invalid" });
    expect(result.status).not.toBe(0);
    expect(result.calls).toContain("notarytool submit");
    expect(result.calls).not.toContain("stapler staple");
  });
});
