import { describe, expect, test } from "vitest";

describe("romanize worker packaging", () => {
  test("uses ES module workers so engine import() can code-split", async () => {
    const { default: createConfig } = await import("../vite.config");
    const config = await createConfig({
      command: "build",
      mode: "production",
    });
    expect(config.worker).toEqual({ format: "es" });
  });
});
