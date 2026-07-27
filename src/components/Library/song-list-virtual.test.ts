import { describe, expect, test } from "vitest";
import {
  createVirtualRowMeasure,
  resolveSongListMeasureElement,
} from "./song-list-virtual";

describe("createVirtualRowMeasure", () => {
  test("returns a height reader for non-Firefox agents", () => {
    const measure = createVirtualRowMeasure(
      "Mozilla/5.0 (Macintosh; Intel Mac OS X) AppleWebKit/537.36 Chrome/120.0.0.0",
    );
    expect(measure).toBeTypeOf("function");
    const height = measure!({
      getBoundingClientRect: () => ({ height: 72 }) as DOMRect,
    } as Element);
    expect(height).toBe(72);
  });

  test("returns undefined on Firefox", () => {
    expect(
      createVirtualRowMeasure(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:122.0) Gecko/20100101 Firefox/122.0",
      ),
    ).toBeUndefined();
  });
});

describe("resolveSongListMeasureElement", () => {
  test("returns undefined without a window", () => {
    expect(
      resolveSongListMeasureElement(false, "Chrome/120.0.0.0"),
    ).toBeUndefined();
  });

  test("delegates to createVirtualRowMeasure when a window exists", () => {
    expect(resolveSongListMeasureElement(true, "Chrome/120.0.0.0")).toBeTypeOf(
      "function",
    );
    expect(
      resolveSongListMeasureElement(
        true,
        "Mozilla/5.0 (Windows NT 10.0; rv:122.0) Gecko/20100101 Firefox/122.0",
      ),
    ).toBeUndefined();
  });
});
