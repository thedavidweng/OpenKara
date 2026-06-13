import { describe, expect, test } from "vitest";

describe("F4: audience plain-text paging listener cleanup when unmount races listen()", () => {
  test("setup function checks cancelled flag after await listen (RED)", async () => {
    const { default: src } =
      await import("./use-audience-plain-text-paging.ts?raw");

    // The listener setup pattern should include a cancelled guard
    // after await listen(), matching the F3 fix pattern.
    // Find the section between "unlisten = await listen" and "void setup()"
    const listenIdx = src.indexOf("unlisten = await listen");
    const setupIdx = src.indexOf("void setup()");
    expect(listenIdx).toBeGreaterThan(-1);
    expect(setupIdx).toBeGreaterThan(listenIdx);

    const afterListen = src.slice(listenIdx, setupIdx);
    expect(afterListen).toContain("if (cancelled)");
    expect(afterListen).toContain("unlisten()");
  });
});
