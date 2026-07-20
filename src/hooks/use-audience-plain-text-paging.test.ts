import { describe, expect, test } from "vitest";

describe("audience plain-text paging listener cleanup when unmount races listen()", () => {
  test("setup function checks cancelled flag after await listen (RED)", async () => {
    const { default: src } =
      await import("./use-audience-plain-text-paging.ts?raw");

    const listenIdx = src.indexOf("unlisten = await listen");
    const setupIdx = src.indexOf("void setup()");
    expect(listenIdx).toBeGreaterThan(-1);
    expect(setupIdx).toBeGreaterThan(listenIdx);

    const afterListen = src.slice(listenIdx, setupIdx);
    expect(afterListen).toContain("if (cancelled)");
    expect(afterListen).toContain("unlisten()");
  });
});
