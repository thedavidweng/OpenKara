import { describe, expect, test } from "vite-plus/test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));

function read(relativePath: string): string {
  return readFileSync(join(ROOT, relativePath), "utf8");
}

describe("product license surfaces declare AGPL-3.0-only", () => {
  test("README badges declare AGPL-3.0-only instead of Apache 2.0", () => {
    const enReadme = read("README.md");
    const zhReadme = read("README_CN.md");

    expect(enReadme).not.toMatch(/License-Apache_2\.0/);
    expect(enReadme).not.toMatch(/License:\s*Apache\s*2\.0/);
    expect(enReadme).toContain("License-AGPL--3.0--only");

    expect(zhReadme).not.toMatch(/License-Apache_2\.0/);
    expect(zhReadme).not.toMatch(/License:\s*Apache\s*2\.0/);
    expect(zhReadme).toContain("License-AGPL--3.0--only");
  });

  test("website LandingPage footer copy declares AGPL-3.0-only", () => {
    const landing = read("website/src/LandingPage.tsx");

    expect(landing).not.toContain("Apache 2.0 License");
    expect(landing).not.toContain("Apache 2.0 许可");
    expect(landing).toContain('footerLicense: "AGPL-3.0-only License"');
    expect(landing).toContain('footerLicense: "AGPL-3.0-only 许可"');
  });

  test("website Terms of Service declare AGPL-3.0-only", () => {
    const enTerms = read("website/terms.md");
    const zhTerms = read("website/zh/terms.md");

    expect(enTerms).not.toMatch(/Apache\s*2\.0/);
    expect(enTerms).toContain(
      "[AGPL-3.0-only](https://github.com/thedavidweng/OpenKara/blob/main/LICENSE)",
    );

    expect(zhTerms).not.toMatch(/Apache\s*2\.0/);
    expect(zhTerms).toContain(
      "[AGPL-3.0-only](https://github.com/thedavidweng/OpenKara/blob/main/LICENSE)",
    );
  });

  test("src-tauri/tauri.conf.json configures bundle license metadata", () => {
    const config = JSON.parse(read("src-tauri/tauri.conf.json"));

    expect(config.bundle?.license).toBe("AGPL-3.0-only");
    expect(config.bundle?.licenseFile).toBe("../LICENSE");
  });
});
